//! Letters as a document library for other front ends (the Flutter app for Android): the
//! desktop's documents, editing logic and page drawing, with no interface of its own. The front
//! end draws everything the user sees and touches; it calls in with what happened (a tap at a
//! point of the view, text typed, a command) and asks for state as data (the pages' places,
//! the caret and selection, the chrome, menus) and for the pages' content as pixels (strips).
//! The egui context kept here is only what the desktop's logic is written against (clipboard
//! output, key types): no pass of egui ever runs. Strings are UTF-8, answers JSON; strings
//! returned are freed with `lt_free_str`.

use std::cell::RefCell;
use std::ffi::{CStr, CString, c_char};
use std::path::PathBuf;

use egui::{Event, pos2, vec2};
use serde_json::{Value, json};

use crate::app::App;

struct Host {
    ctx: egui::Context,
    app: App,
    start: std::time::Instant,
    /// Text the last command copied (Copy, Cut), for the system clipboard.
    copied: Option<String>,
}

thread_local! {
    static HOST: RefCell<Option<Host>> = const { RefCell::new(None) };
    static ERR: RefCell<String> = const { RefCell::new(String::new()) };
}

/// Runs `f` on the app, then the canvas's housekeeping; a panic is caught (and logged), not
/// carried into the front end.
fn with<T: Default>(f: impl FnOnce(&mut Host) -> T) -> T {
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        HOST.with(|h| match h.try_borrow_mut() {
            Ok(mut g) => match g.as_mut() {
                Some(h) => {
                    let r = f(h);
                    // a web video looked up in the background
                    crate::media::web_poll(&mut h.app);
                    // tabs waiting to close (a changed one asks first: dlgapi "unsaved")
                    crate::tabs::closing_step(&mut h.app, &h.ctx);
                    h.app.refresh();
                    crate::canvas::settle(&mut h.app);
                    // what the commands put on the clipboard
                    let copied = h.ctx.output_mut(|o| {
                        let mut t = None;
                        o.commands.retain(|c| match c {
                            egui::OutputCommand::CopyText(s) => {
                                t = Some(s.clone());
                                false
                            }
                            _ => false,
                        });
                        t
                    });
                    if copied.is_some() {
                        h.copied = copied;
                    }
                    r
                }
                None => T::default(),
            },
            Err(_) => T::default(),
        })
    }));
    r.unwrap_or_default()
}

fn out(s: String) -> *mut c_char {
    CString::new(s.replace('\0', "")).map(CString::into_raw).unwrap_or(std::ptr::null_mut())
}

fn json_out(v: Option<Value>) -> *mut c_char {
    out(v.unwrap_or(Value::Null).to_string())
}

fn arg(p: *const c_char) -> String {
    if p.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

/// The app's files: HOME and the XDG folders under `path` (the app's private storage).
#[unsafe(no_mangle)]
pub extern "C" fn lt_home(path: *const c_char) {
    #[cfg(target_os = "android")]
    kn_ui::android::init_logging("Letters");
    let home = PathBuf::from(arg(path));
    for (k, sub) in [("HOME", ""), ("XDG_CONFIG_HOME", "config"), ("XDG_DATA_HOME", "data"), ("XDG_CACHE_HOME", "cache")] {
        let d = if sub.is_empty() { home.clone() } else { home.join(sub) };
        let _ = std::fs::create_dir_all(&d);
        // (before the app starts any thread)
        unsafe { std::env::set_var(k, d) };
    }
}

/// A web page to open: the front end opens it (the system's way from there); without one
/// (the desktop), kn_ui's.
static URLS: std::sync::Mutex<Option<Vec<String>>> = std::sync::Mutex::new(None);

pub fn open_url(url: &str) {
    match URLS.lock().ok().as_mut().and_then(|q| q.as_mut()) {
        Some(q) => q.push(url.to_string()),
        None => kn_ui::open_url(url),
    }
}

/// The Content Hub's catalogue, searches and downloads (kn_ui contenthub::api). Holds no
/// document, so a worker thread may call it while the window works: "catalog", "lib" (an
/// index), "libSearch" / "query" (what's typed), "search" ({words, offset}), "thumb" (a url:
/// its cached file), "download" ({item, dir}).
#[unsafe(no_mangle)]
pub extern "C" fn lt_hub(kind: *const c_char, a: *const c_char) -> *mut c_char {
    use kn_ui::contenthub::api;
    let (kind, a) = (arg(kind), arg(a));
    let r = std::panic::catch_unwind(|| {
        let v: Value = serde_json::from_str(&a).unwrap_or(Value::Null);
        match kind.as_str() {
            "catalog" => api::catalog(),
            "lib" => api::lib_shape(a.parse().unwrap_or(usize::MAX)),
            "libSearch" => api::lib_search(&a),
            "query" => Value::String(api::query(&a)),
            "search" => api::search_page(v["words"].as_str().unwrap_or_default(), v["offset"].as_u64().unwrap_or(0) as usize),
            "thumb" => kn_ui::contenthub::thumb_file(&a).map(|p| Value::String(p.to_string_lossy().into_owned())).unwrap_or(Value::Null),
            "download" => api::download_to(&v["item"], std::path::Path::new(v["dir"].as_str().unwrap_or_default())),
            _ => Value::Null,
        }
    });
    json_out(r.ok())
}

/// Starts Letters with a new blank document; the view is `width` × `height` points.
#[unsafe(no_mangle)]
pub extern "C" fn lt_init(width: f32, height: f32) {
    let _ = std::panic::catch_unwind(|| {
        pg_core::init();
        let ctx = egui::Context::default();
        let mut app = App::with_ctx(&ctx, Vec::new());
        // (the template chooser at launch as the desktop's, but not in automated checks, as CS_SNAP)
        if std::env::var_os("LETTERS_SNAP").is_some() && std::env::var_os("CS_WELCOME").is_none() {
            app.chooser = false;
        }
        // (the front end opens web pages)
        if let Ok(mut q) = URLS.lock() {
            *q = Some(Vec::new());
        }
        // (opening at Fit Width; it's edge to edge everywhere)
        app.canvas.open_fit_width = true;
        app.canvas.fit = crate::canvas::Fit::Width;
        crate::canvas::set_view(&mut app, width, height);
        HOST.with(|h| *h.borrow_mut() = Some(Host { ctx, app, start: std::time::Instant::now(), copied: None }));
    });
    with(|_| ());
}

/// The pages' view changed size (points).
#[unsafe(no_mangle)]
pub extern "C" fn lt_view(width: f32, height: f32) {
    with(|h| crate::canvas::set_view(&mut h.app, width, height))
}

/// The inspector as data: which pane shows and its controls' values (inspapi::state).
#[unsafe(no_mangle)]
pub extern "C" fn lt_inspector() -> *mut c_char {
    json_out(with(|h| Some(crate::inspapi::state(&mut h.app))))
}

/// Changes an inspector control: `key` as lt_inspector names it, `value` JSON.
#[unsafe(no_mangle)]
pub extern "C" fn lt_inspector_set(key: *const c_char, value: *const c_char) {
    let (k, v) = (arg(key), arg(value));
    let v: Value = serde_json::from_str(&v).unwrap_or(Value::Null);
    with(|h| match k.strip_prefix("dlg.") {
        Some(k) => crate::dlgapi::set(&mut h.app, &h.ctx, k, &v),
        None => crate::inspapi::set(&mut h.app, &k, &v),
    })
}

/// A picture of the document's (its data id) as a small PNG (the inspector's style previews and
/// gallery list): its length into `len`; freed with lt_free_bytes.
#[unsafe(no_mangle)]
pub extern "C" fn lt_thumbnail(data: u64, max: u32, len: *mut u32) -> *mut u8 {
    let png: Vec<u8> = with(|h| {
        let Some(bytes) = h.app.wb.as_ref().and_then(|w| w.doc.data(data)) else { return Vec::new() };
        let Ok(img) = image::load_from_memory(&bytes) else { return Vec::new() };
        let t = img.thumbnail(max, max);
        let mut out = std::io::Cursor::new(Vec::new());
        if t.write_to(&mut out, image::ImageFormat::Png).is_err() {
            return Vec::new();
        }
        out.into_inner()
    });
    unsafe { *len = png.len() as u32 };
    if png.is_empty() {
        return std::ptr::null_mut();
    }
    Box::into_raw(png.into_boxed_slice()) as *mut u8
}

#[unsafe(no_mangle)]
pub extern "C" fn lt_free_bytes(p: *mut u8, len: u32) {
    if !p.is_null() {
        drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(p, len as usize)) });
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn lt_free_str(s: *mut c_char) {
    if !s.is_null() {
        drop(unsafe { CString::from_raw(s) });
    }
}

/// The last error's message.
#[unsafe(no_mangle)]
pub extern "C" fn lt_error() -> *mut c_char {
    out(ERR.with(|e| e.borrow().clone()))
}

/// Opens a document (as File > Open: Pages, Word, RTF, Markdown, text).
#[unsafe(no_mangle)]
pub extern "C" fn lt_open(path: *const c_char) -> i32 {
    let p = PathBuf::from(arg(path));
    with(|h| {
        // (as File > Open on the desktop: in its own tab, or the tab already showing it)
        crate::tabs::open_in_tab(&mut h.app, &p);
        0
    })
}

/// A file being read on another thread for `lt_open_begin` / `lt_open_poll` (the file, what was
/// read once done, and whether it was cancelled).
static OPENING: std::sync::Mutex<Option<(PathBuf, Option<crate::app::ReadFile>)>> = std::sync::Mutex::new(None);

/// Starts opening `path` as `lt_open` would, reading it on another thread so the front end can
/// show its "Opening…" panel; `lt_open_poll` finishes it.
#[unsafe(no_mangle)]
pub extern "C" fn lt_open_begin(path: *const c_char) {
    let p = PathBuf::from(arg(path));
    *OPENING.lock().unwrap() = Some((p.clone(), None));
    std::thread::spawn(move || {
        let read = crate::app::ReadFile::read(&p);
        if let Some(o) = OPENING.lock().unwrap().as_mut().filter(|o| o.0 == p) {
            o.1 = Some(read);
        }
    });
}

/// 1 while the file `lt_open_begin` started is still being read; 0 once it's open (or there's
/// nothing being opened).
#[unsafe(no_mangle)]
pub extern "C" fn lt_open_poll() -> i32 {
    let mut g = OPENING.lock().unwrap();
    if g.as_ref().is_some_and(|o| o.1.is_none()) {
        return 1;
    }
    let Some((p, read)) = g.take() else { return 0 };
    drop(g);
    with(|h| crate::tabs::open_read_in_tab(&mut h.app, &p, read));
    0
}

/// Cancel: what `lt_open_begin` is reading is dropped when it's done.
#[unsafe(no_mangle)]
pub extern "C" fn lt_open_cancel() {
    *OPENING.lock().unwrap() = None;
}

/// Saves to `path` (.pages, or .docx in Word mode) as File > Save would.
#[unsafe(no_mangle)]
pub extern "C" fn lt_save(path: *const c_char) -> i32 {
    let p = PathBuf::from(arg(path));
    with(|h| {
        if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("docx")) {
            let hybrid = h.app.word.as_ref().map(|w| w.hybrid).unwrap_or(true);
            h.app.save_word(&p, hybrid);
        } else {
            h.app.word = None;
            h.app.save_to(&p);
        }
        0
    })
}

/// Exports to `path` by its extension (pdf, rtf, txt, epub, html, md), as File > Export To.
#[unsafe(no_mangle)]
pub extern "C" fn lt_export(path: *const c_char) -> i32 {
    let p = PathBuf::from(arg(path));
    with(|h| {
        crate::text::finish_typing(&mut h.app);
        let Some(w) = h.app.wb.as_ref() else { return -1 };
        match pg_core::export::export(w, &p) {
            Ok(()) => 0,
            Err(e) => {
                ERR.with(|x| *x.borrow_mut() = e);
                -1
            }
        }
    })
}

// ---- the pages ----

/// Where the view is: scroll (document points at its top-left) and zoom, as the front end
/// scrolls and zooms it.
#[unsafe(no_mangle)]
pub extern "C" fn lt_scroll(x: f32, y: f32, zoom: f32) {
    with(|h| {
        let c = &mut h.app.canvas;
        c.scroll = vec2(x, y);
        if (c.zoom - zoom).abs() > 1e-4 {
            c.zoom = zoom.clamp(0.1, 4.0);
            c.fit = crate::canvas::Fit::None;
        }
    })
}

/// The pages' places: {"zoom", "scroll" (after the engine moved it: the caret kept in view, a
/// page asked for), "size" (document points), "pages": [[x, y, w, h]…], "strip" (points),
/// "version", "pageInView"}.
#[unsafe(no_mangle)]
pub extern "C" fn lt_geometry() -> *mut c_char {
    json_out(with(|h| {
        let app = &mut h.app;
        let l = app.layout.as_ref()?;
        let pages: Vec<Value> = (0..l.page_count())
            .map(|i| {
                let r = crate::canvas::page_rect(&app.canvas, l, i);
                json!([r.min.x, r.min.y, r.width(), r.height()])
            })
            .collect();
        let size = crate::canvas::doc_size(&app.canvas, l);
        Some(json!({ "zoom": app.canvas.zoom, "scroll": [app.canvas.scroll.x, app.canvas.scroll.y], "size": [size.x, size.y], "pages": pages, "strip": crate::canvas::STRIP_PT, "version": app.version, "pageInView": app.canvas.page_in_view, "fit": match app.canvas.fit { crate::canvas::Fit::Width => "width", crate::canvas::Fit::Page => "page", crate::canvas::Fit::None => "none" } }))
    }))
}

/// Strip `strip` of page `page`: a key of what it shows (it changes when it does).
#[unsafe(no_mangle)]
pub extern "C" fn lt_strip_key(page: u32, strip: u32) -> u64 {
    with(|h| crate::canvas::strip_content_key(&mut h.app, page as usize, strip as i32))
}

/// Strip `strip` of page `page` at `scale` pixels per point into `buf` (`w` × `h`, RGBA
/// premultiplied; w = ceil(page width × scale), h = ceil(strip × scale)).
#[unsafe(no_mangle)]
pub extern "C" fn lt_render_strip(page: u32, strip: u32, scale: f32, buf: *mut u8, w: u32, h: u32) -> i32 {
    with(|host| {
        let Some(pm) = crate::canvas::render_strip_pixmap(&mut host.app, page as usize, strip as i32, scale) else { return -1 };
        let out = unsafe { std::slice::from_raw_parts_mut(buf, (w * h * 4) as usize) };
        let (pw, ph) = (pm.width().min(w) as usize, pm.height().min(h) as usize);
        let src = pm.data();
        for y in 0..ph {
            let s0 = y * pm.width() as usize * 4;
            let d0 = y * w as usize * 4;
            out[d0..d0 + pw * 4].copy_from_slice(&src[s0..s0 + pw * 4]);
        }
        0
    })
}

/// What's drawn over the pages: the caret, the selection, selected objects (frame, handles),
/// the cells selected, a drag going on (canvas::overlays).
#[unsafe(no_mangle)]
pub extern "C" fn lt_overlays() -> *mut c_char {
    json_out(with(|h| Some(crate::canvas::overlays(&mut h.app))))
}

/// The newest frame of the movie playing in place (media::play through lt_action "mediaplay"):
/// copied into `buf` (`cap` bytes, RGBA) with its size in `w` / `h`; returns the drawable's id,
/// 0 when no new frame came (or it doesn't fit).
#[unsafe(no_mangle)]
pub extern "C" fn lt_media_frame(buf: *mut u8, cap: u32, w: *mut u32, h: *mut u32) -> u64 {
    with(|h_| {
        let ctx = h_.ctx.clone();
        let Some((id, size, rgba)) = crate::media::frame(&mut h_.app, &ctx) else { return 0 };
        if buf.is_null() || rgba.len() > cap as usize || w.is_null() || h.is_null() {
            return 0;
        }
        unsafe {
            std::ptr::copy_nonoverlapping(rgba.as_ptr(), buf, rgba.len());
            *w = size[0] as u32;
            *h = size[1] as u32;
        }
        id
    })
}

/// A moving object alone (lt_overlays' live objects) at `scale` pixels per point into `buf`
/// (`w` × `h` RGBA premultiplied; w = ceil(box width × scale), h = ceil(box height × scale)).
#[unsafe(no_mangle)]
pub extern "C" fn lt_render_object(id: u64, scale: f32, buf: *mut u8, w: u32, h: u32) -> i32 {
    with(|host| {
        let Some(pm) = crate::canvas::render_object_pixmap(&mut host.app, id as pg_core::kn_format::ObjId, scale) else { return -1 };
        let out = unsafe { std::slice::from_raw_parts_mut(buf, (w * h * 4) as usize) };
        let (pw, ph) = (pm.width().min(w) as usize, pm.height().min(h) as usize);
        let src = pm.data();
        for y in 0..ph {
            let s0 = y * pm.width() as usize * 4;
            let d0 = y * w as usize * 4;
            out[d0..d0 + pw * 4].copy_from_slice(&src[s0..s0 + pw * 4]);
        }
        0
    })
}

/// The pointer on the view (points from its top-left): kind 0 moved (held), 1 pressed, 2
/// released; button 1 the secondary (the context menu's selection rules); Shift / Ctrl held.
/// Repeated presses at one place within 0.45 s are double and triple clicks, as on the desktop.
#[unsafe(no_mangle)]
pub extern "C" fn lt_pointer(kind: u32, x: f32, y: f32, button: u32, shift: bool, ctrl: bool) {
    with(|h| {
        let mods = egui::Modifiers { shift, ctrl, command: ctrl, ..Default::default() };
        let secondary = button == 1 && kind == 1;
        let p = crate::canvas::PointerIn {
            pressed: kind == 1 && !secondary,
            released: kind == 2 && button == 0,
            down: kind == 0 || (kind == 1 && !secondary),
            secondary,
            pos: Some(pos2(x, y)),
            mods,
            time: h.start.elapsed().as_secs_f64(),
            in_canvas: true,
        };
        if secondary {
            // the Add Guide items go where the menu opened
            let at = h.app.layout.as_ref().and_then(|l| h.app.canvas.hit_page(l, pos2(x, y)));
            crate::guides::set_menu_at(at);
        }
        crate::canvas::pointer(&mut h.app, &p);
    })
}

/// The right-click menu's items for what's selected now: [{label, action, enabled, bold,
/// checked} | {separator} | {label, note} | {label, kids}].
#[unsafe(no_mangle)]
pub extern "C" fn lt_context_menu() -> *mut c_char {
    json_out(with(|h| Some(Value::Array(crate::menus::context_items(&h.app).iter().map(|i| i.to_json()).collect()))))
}

/// A toolbar popover's contents (View, Insert, Media: menu items; Shape: the shapes' paths).
#[unsafe(no_mangle)]
pub extern "C" fn lt_popover(id: *const c_char) -> *mut c_char {
    let id = arg(id);
    json_out(with(|h| Some(crate::menus::popover_items(&h.app, &id))))
}

/// The selected table for the canvas's table chrome (tables::api_state), null if none.
#[unsafe(no_mangle)]
pub extern "C" fn lt_table() -> *mut c_char {
    json_out(with(|h| Some(crate::tables::api_state(&h.app))))
}

/// A row (`rows`) or column tab's menu items.
#[unsafe(no_mangle)]
pub extern "C" fn lt_table_tab_menu(rows: bool, i: u32) -> *mut c_char {
    json_out(with(|h| Some(Value::Array(crate::tables::tab_menu_items(&h.app, rows, i).iter().map(|m| m.to_json()).collect()))))
}

/// The sidebar (sidebar::api_state).
#[unsafe(no_mangle)]
pub extern "C" fn lt_sidebar() -> *mut c_char {
    json_out(with(|h| Some(crate::sidebar::api_state(&h.app))))
}

/// Page `page` whole at `scale` pixels a point into `buf` (`w` × `h` RGBA premultiplied): the
/// sidebar's thumbnails.
#[unsafe(no_mangle)]
pub extern "C" fn lt_render_page(page: u32, scale: f32, buf: *mut u8, w: u32, h: u32) -> i32 {
    with(|host| {
        let Some(pm) = crate::sidebar::render_page(&mut host.app, page as usize, scale) else { return -1 };
        let out = unsafe { std::slice::from_raw_parts_mut(buf, (w * h * 4) as usize) };
        let (pw, ph) = (pm.width().min(w) as usize, pm.height().min(h) as usize);
        let src = pm.data();
        for y in 0..ph {
            let s0 = y * pm.width() as usize * 4;
            let d0 = y * w as usize * 4;
            out[d0..d0 + pw * 4].copy_from_slice(&src[s0..s0 + pw * 4]);
        }
        0
    })
}

/// Runs a menu item's action (lt_context_menu's).
#[unsafe(no_mangle)]
pub extern "C" fn lt_action(action: *const c_char) {
    let a = arg(action);
    with(|h| {
        let ctx = h.ctx.clone();
        crate::menus::menu_action(&mut h.app, &ctx, &a);
    })
}

// ---- typing ----

fn keyboard(h: &mut Host, events: Vec<Event>, command: bool) {
    let ctx = h.ctx.clone();
    let t = h.start.elapsed().as_secs_f64();
    crate::text::keyboard(&mut h.app, &ctx, events, t, command);
}

/// Typed text (the on-screen keyboard), as typed on the desktop.
#[unsafe(no_mangle)]
pub extern "C" fn lt_text(text: *const c_char) {
    let t = arg(text);
    with(|h| keyboard(h, vec![Event::Text(t)], false))
}

/// A key: its name as egui's (Backspace, Delete, Enter, Tab, ArrowLeft, Home…), with Shift,
/// Ctrl and Alt. Ctrl with a letter is a menu shortcut (as the desktop's menus show them).
#[unsafe(no_mangle)]
pub extern "C" fn lt_key(name: *const c_char, shift: bool, ctrl: bool, alt: bool) {
    let name = arg(name);
    with(|h| {
        if name == "Tab" && !ctrl {
            crate::text::press_tab(shift);
            return keyboard(h, Vec::new(), false);
        }
        let Some(key) = egui::Key::from_name(&name) else { return };
        let modifiers = egui::Modifiers { shift, ctrl, alt, command: ctrl, ..Default::default() };
        if ctrl && !matches!(key, egui::Key::ArrowLeft | egui::Key::ArrowRight | egui::Key::ArrowUp | egui::Key::ArrowDown | egui::Key::Home | egui::Key::End | egui::Key::Backspace | egui::Key::Delete) {
            let ctx = h.ctx.clone();
            if let Some(cmd) = crate::menus::shortcut_command(modifiers, key) {
                crate::menus::run(&mut h.app, &ctx, &cmd);
            }
            return;
        }
        keyboard(h, vec![Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }], ctrl);
    })
}

/// Pasted text (the system clipboard), as Edit > Paste.
#[unsafe(no_mangle)]
pub extern "C" fn lt_paste(text: *const c_char) {
    let t = arg(text);
    with(|h| keyboard(h, vec![Event::Paste(t)], false))
}

/// Text the last commands copied (Copy, Cut), once; null if none.
#[unsafe(no_mangle)]
pub extern "C" fn lt_copied() -> *mut c_char {
    match with(|h| h.copied.take()) {
        Some(s) => out(s),
        None => std::ptr::null_mut(),
    }
}

// ---- commands and state ----

/// The menu bar as data: [{label, command, enabled, checked, shortcut, separator, kids}…], as
/// the desktop shows it now (labels like "Show Ruler" / "Hide Ruler" follow the state).
#[unsafe(no_mangle)]
pub extern "C" fn lt_menus() -> *mut c_char {
    json_out(with(|h| Some(crate::menus::as_data(&h.app))))
}

/// Runs a menu command by its name (as lt_menus gives it).
#[unsafe(no_mangle)]
pub extern "C" fn lt_run(command: *const c_char) {
    let c = arg(command);
    with(|h| {
        let ctx = h.ctx.clone();
        crate::menus::run(&mut h.app, &ctx, &c);
    })
}

/// A toolbar item without a popover (Add Page, Text, Comment, Format, Document…), by its id.
#[unsafe(no_mangle)]
pub extern "C" fn lt_toolbar(id: *const c_char) {
    let id = arg(id);
    with(|h| {
        let ctx = h.ctx.clone();
        crate::toolbar::press(&mut h.app, &ctx, &id);
    })
}

/// The document's state: title, edited, the message shown, undo / redo, the inspector open,
/// typing.
#[unsafe(no_mangle)]
pub extern "C" fn lt_status() -> *mut c_char {
    json_out(with(|h| {
        let a = &h.app;
        Some(json!({
            "title": a.title(),
            "edited": a.doc().is_some_and(|d| d.dirty),
            "message": a.message.as_ref().map(|m| m.0.clone()),
            "canUndo": a.hist.can_undo(),
            "canRedo": a.hist.can_redo(),
            "inspector": a.inspector,
            "inspectorMode": if a.insp_mode == crate::app::InspMode::Document { "Document" } else { "Format" },
            "editingText": a.cursor.is_some(),
            "objects": a.selected_objs().len(),
            "find": crate::find::api_state(a),
            "ruler": if a.ruler { crate::ruler::api_state(a) } else { Value::Null },
            "wordCount": if a.word_count { Value::String(crate::notes::word_count_label(a)) } else { Value::Null },
            "screenView": a.screen_view,
            "notes": crate::notes::api_state(a),
            "tabs": crate::tabs::api_state(a),
            "cells": a.sel.is_some(),
            "windows": crate::dlgapi::state(a),
            "savedColors": a.settings.saved_colors,
            "openUrls": URLS.lock().ok().and_then(|mut q| q.as_mut().map(std::mem::take)).unwrap_or_default(),
            // (something finishes in the background: the front end asks again soon)
            "busy": crate::media::web_state().1 || crate::media::record_state().flatten().is_some() || crate::chooserapi::pending(),
        }))
    }))
}

/// The window's chrome as data, for the front end to draw itself: the look's colours (the
/// desktop's palette, so both draw alike), the title and the toolbar's items and states.
#[unsafe(no_mangle)]
pub extern "C" fn lt_chrome() -> *mut c_char {
    out(with(|h| {
        let a = &h.app;
        let c = |c: egui::Color32| {
            let [r, g, b, al] = c.to_srgba_unmultiplied();
            (u32::from(al) << 24) | (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
        };
        use kn_ui::theme;
        let vis = h.ctx.style_of(if theme::is_light() { egui::Theme::Light } else { egui::Theme::Dark }).visuals.clone();
        let grays: serde_json::Map<String, Value> = (0..=255u8).map(|v| (format!("{v:02x}"), json!(c(theme::gray(v))))).collect();
        let items: Vec<Value> = crate::toolbar::items(a)
            .iter()
            .map(|id| {
                let d = crate::toolbar::def(id);
                json!({
                    "id": id,
                    "label": d.map(|d| d.label).unwrap_or(""),
                    "tip": d.map(|d| d.tip).unwrap_or(""),
                    "icon": d.map(|d| format!("{:?}", d.icon)).unwrap_or_default(),
                    "enabled": crate::toolbar::enabled(a, id),
                    "pressed": crate::toolbar::pressed(a, id),
                    "popup": crate::toolbar::has_popup(id),
                })
            })
            .collect();
        let dirty = a.doc().is_some_and(|d| d.dirty);
        let mode = a.word.as_ref().map(|w| w.label().to_string());
        let sub = match (mode, dirty) {
            (Some(m), true) => Some(format!("{m} · Edited")),
            (Some(m), false) => Some(m),
            (None, true) => Some("Edited".to_string()),
            (None, false) => None,
        };
        Some(json!({
            "light": theme::is_light(),
            "colors": {
                "text": c(theme::TEXT()),
                "textDim": c(theme::TEXT_DIM()),
                "separator": c(theme::SEPARATOR()),
                "panel": c(theme::PANEL_BG()),
                "sidebar": c(theme::SIDEBAR_BG()),
                "control": c(theme::CONTROL_BG()),
                "accent": c(theme::ACCENT()),
                "header": c(crate::look::header_bg()),
                "canvas": c(crate::look::canvas_bg()),
                "capsule": c(crate::look::capsule_fill()),
                "styleButton": c(crate::look::style_button_colors().0),
                "styleButtonText": c(crate::look::style_button_colors().1),
                "controlStroke": c(if theme::is_light() { theme::tint(egui::Color32::from_rgb(0xcc, 0xcc, 0xcc)) } else { egui::Color32::TRANSPARENT }),
                "gray": grays,
                // egui's own greys as the theme tints them (window title bars, hovered rows,
                // window outlines, text fields, disabled text)
                "titleBar": c(vis.widgets.open.weak_bg_fill),
                "hover": c(vis.widgets.hovered.weak_bg_fill),
                "windowStroke": c(vis.window_stroke.color),
                "field": c(vis.text_edit_bg_color()),
                "fieldStroke": c(vis.widgets.inactive.bg_stroke.color),
                "weakText": c(vis.weak_text_color()),
                // liquid glass's surface tint and Pages' tint under the floating toolbar
                "glassTint": c(theme::tint(if theme::is_light() { egui::Color32::from_rgb(0xfa, 0xfa, 0xfa) } else { egui::Color32::from_rgb(0x2a, 0x2a, 0x2c) })),
                "toolbarTint": c(kn_ui::glass::ToolbarBackground::tint()),
                // the document tab bar (tabs.rs bar): its track, the active tab, the lines between
                "tabBar": c(if theme::is_light() { theme::tint(egui::Color32::from_rgb(0xe8, 0xe8, 0xe8)) } else { theme::tint(egui::Color32::from_rgb(0x35, 0x35, 0x35)) }),
                "tabActive": c(if theme::is_light() { theme::tint(egui::Color32::WHITE) } else { theme::tint(egui::Color32::from_rgb(0x4a, 0x4a, 0x4a)) }),
                "tabLine": c(if theme::is_light() { theme::tint(egui::Color32::from_gray(0xc8)) } else { theme::tint(egui::Color32::from_gray(0x48)) }),
            },
            "title": a.title(),
            "sub": sub,
            "zoom": a.canvas.zoom,
            "showToolbar": a.show_toolbar,
            // Settings' look: liquid glass, translucent inspector, the classic toolbar strip, what
            // shows under the floating toolbar, the scale (percent, 0 the system's) and the layout
            "glass": a.look.liquid_glass,
            "translucent": a.look.translucent_panes,
            "classic": a.look.classic_look,
            "toolbarBackground": a.look.toolbar_background.key(),
            "uiScale": a.settings.ui_scale,
            "uiLayout": a.settings.ui_layout,
            "android": cfg!(target_os = "android"),
            "toolbarMode": a.settings.toolbar_mode,
            "toolbar": items,
        }))
    })
    .unwrap_or(Value::Null)
    .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(p: *mut c_char) -> Value {
        let v = serde_json::from_str(&unsafe { CStr::from_ptr(p) }.to_string_lossy()).unwrap_or(Value::Null);
        lt_free_str(p);
        v
    }

    /// Tap into the body, type, select, a menu command, the context menu, a strip drawn: all
    /// without any egui pass.
    #[test]
    fn document_api() {
        let home = std::env::temp_dir().join("letters-ffi-test");
        let c = CString::new(home.to_string_lossy().as_bytes()).unwrap();
        lt_home(c.as_ptr());
        lt_init(800.0, 1000.0);
        let g = json(lt_geometry());
        let page = g["pages"][0].as_array().unwrap().iter().map(|v| v.as_f64().unwrap() as f32).collect::<Vec<_>>();
        let zoom = g["zoom"].as_f64().unwrap() as f32;
        let scroll = (g["scroll"][0].as_f64().unwrap() as f32, g["scroll"][1].as_f64().unwrap() as f32);
        // a tap in the body (100 points into the page)
        let (x, y) = ((page[0] + 100.0 - scroll.0) * zoom, (page[1] + 100.0 - scroll.1) * zoom);
        lt_pointer(1, x, y, 0, false, false);
        lt_pointer(2, x, y, 0, false, false);
        let t = CString::new("Hello world").unwrap();
        lt_text(t.as_ptr());
        let o = json(lt_overlays());
        assert!(o["caret"].is_array(), "{o}");
        let k = CString::new("ArrowLeft").unwrap();
        lt_key(k.as_ptr(), true, false, false);
        let o = json(lt_overlays());
        assert!(!o["rects"].as_array().unwrap().is_empty(), "a selection: {o}");
        let s = json(lt_status());
        assert_eq!(s["canUndo"], true);
        let m = json(lt_context_menu());
        assert!(m.as_array().unwrap().iter().any(|i| i["label"] == "Copy"), "{m}");
        let cmd = CString::new("Copy").unwrap();
        lt_run(cmd.as_ptr());
        let copied = lt_copied();
        assert!(!copied.is_null());
        assert_eq!(unsafe { CStr::from_ptr(copied) }.to_string_lossy(), "d");
        lt_free_str(copied);
        let key = lt_strip_key(0, 0);
        assert_ne!(key, 0);
        let (w, h) = ((page[2] * 2.0).ceil() as u32, (96.0f32 * 2.0).ceil() as u32);
        let mut buf = vec![0u8; (w * h * 4) as usize];
        assert_eq!(lt_render_strip(0, 0, 2.0, buf.as_mut_ptr(), w, h), 0);
        assert!(buf.chunks(4).any(|p| p[3] > 0 && p[0] < 128), "text drawn in the first strip");
        // typing changes the strip's key
        let t = CString::new("!").unwrap();
        lt_text(t.as_ptr());
        assert_ne!(lt_strip_key(0, 0), key);
        let ch = json(lt_chrome());
        assert!(ch["toolbar"].as_array().unwrap().len() > 5);
    }
}
