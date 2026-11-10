//! Values as a document library for other front ends (the Flutter app for Android): the
//! desktop's spreadsheets, editing logic and sheet drawing, with no interface of its own. The
//! front end draws everything the user sees and touches; it calls in with what happened (a tap
//! at a point of the view, text typed, a command) and asks for state as data (the view, the
//! selection, the table's chrome, the chrome, menus, the inspector) and for the sheet as pixels
//! (tiles). The egui context kept here is only what the desktop's logic is written against
//! (clipboard output, key types): no pass of egui ever runs. Strings are UTF-8, answers JSON;
//! strings returned are freed with `vl_free_str`.

use std::cell::RefCell;
use std::ffi::{CStr, CString, c_char};
use std::path::PathBuf;

use egui::pos2;
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

/// Runs `f` on the app, then the sheet's housekeeping; a panic is caught (and logged), not
/// carried into the front end.
fn with<T: Default>(f: impl FnOnce(&mut Host) -> T) -> T {
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        HOST.with(|h| match h.try_borrow_mut() {
            Ok(mut g) => match g.as_mut() {
                Some(h) => {
                    let r = f(h);
                    crate::api::settle(&mut h.app);
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
pub extern "C" fn vl_home(path: *const c_char) {
    #[cfg(target_os = "android")]
    kn_ui::android::init_logging("Values");
    let home = PathBuf::from(arg(path));
    // (the user's own home, for the media browser's ~/Pictures and ~/Music)
    if let Some(h) = std::env::var_os("HOME").filter(|_| std::env::var_os("VALUES_USER_HOME").is_none()) {
        unsafe { std::env::set_var("VALUES_USER_HOME", h) };
    }
    for (k, sub) in [("HOME", ""), ("XDG_CONFIG_HOME", "config"), ("XDG_DATA_HOME", "data"), ("XDG_CACHE_HOME", "cache")] {
        let d = if sub.is_empty() { home.clone() } else { home.join(sub) };
        let _ = std::fs::create_dir_all(&d);
        // (before the app starts any thread)
        unsafe { std::env::set_var(k, d) };
    }
}

thread_local! {
    /// The media browser was listed since it opened.
    static BROWSER_LISTED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Web pages to open: the front end opens them (None: no front end, the desktop's way).
static URLS: std::sync::Mutex<Option<Vec<String>>> = std::sync::Mutex::new(None);

/// The Content Hub's catalogue, searches and downloads (kn_ui contenthub::api). Holds no
/// document, so a worker thread may call it while the window works: "catalog", "lib" (an
/// index), "libSearch" / "query" (what's typed), "search" ({words, offset}), "thumb" (a url:
/// its cached file), "download" ({item, dir}).
#[unsafe(no_mangle)]
pub extern "C" fn vl_hub(kind: *const c_char, a: *const c_char) -> *mut c_char {
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

/// A picture of the document's (its data id) as a small PNG (the inspector's previews): its
/// length into `len`; freed with vl_free_bytes.
#[unsafe(no_mangle)]
pub extern "C" fn vl_thumbnail(data: u64, max: u32, len: *mut u32) -> *mut u8 {
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
pub extern "C" fn vl_free_bytes(p: *mut u8, len: u32) {
    if !p.is_null() {
        drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(p, len as usize)) });
    }
}

/// Starts Values with a new blank spreadsheet; the view is `width` × `height` points.
#[unsafe(no_mangle)]
pub extern "C" fn vl_init(width: f32, height: f32) {
    let _ = std::panic::catch_unwind(|| {
        nm_core::init();
        // Learn Spelling keeps Values' words in its own config
        if let Some(base) = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from) {
            kn_ui::spell::set_words_file(base.join("numdeck").join("words.txt"));
        }
        let ctx = egui::Context::default();
        // (the front end owns the clipboard: copies reach it as text)
        kn_ui::clipboard::set_hosted(true);
        let mut app = App::with_ctx(&ctx, None);
        // the template chooser at launch, as the desktop's (not in automated checks, unless
        // VALUES_WELCOME asks for it)
        app.templates.chooser = std::env::var_os("VALUES_SNAP").is_none() || std::env::var_os("VALUES_WELCOME").is_some();
        if let Ok(mut q) = URLS.lock() {
            *q = Some(Vec::new());
        }
        crate::api::set_view(&mut app, width, height);
        HOST.with(|h| *h.borrow_mut() = Some(Host { ctx, app, start: std::time::Instant::now(), copied: None }));
    });
    with(|_| ());
}

/// The sheet's view changed size (points).
#[unsafe(no_mangle)]
pub extern "C" fn vl_view(width: f32, height: f32) {
    with(|h| crate::api::set_view(&mut h.app, width, height))
}

#[unsafe(no_mangle)]
pub extern "C" fn vl_free_str(s: *mut c_char) {
    if !s.is_null() {
        drop(unsafe { CString::from_raw(s) });
    }
}

/// The last error's message.
#[unsafe(no_mangle)]
pub extern "C" fn vl_error() -> *mut c_char {
    out(ERR.with(|e| e.borrow().clone()))
}

/// Opens a spreadsheet (as File > Open: Numbers, Excel, CSV / TSV). Non-zero when it didn't
/// (the message in vl_error), or when it asks for a password (vl_status windows).
#[unsafe(no_mangle)]
pub extern "C" fn vl_open(path: *const c_char) -> i32 {
    let p = PathBuf::from(arg(path));
    with(|h| {
        crate::canvas::commit(&mut h.app);
        h.app.templates.chooser = false;
        let before = h.app.version;
        h.app.open(&p);
        if h.app.version == before {
            ERR.with(|e| *e.borrow_mut() = h.app.message.as_ref().map(|m| m.0.clone()).unwrap_or_else(|| "it isn't a spreadsheet Values can read".into()));
            return if h.app.password.is_some() { 0 } else { -1 };
        }
        0
    })
}

thread_local! {
    /// The spreadsheet vl_open_start is reading on a thread of its own.
    static OPENING: RefCell<Option<(PathBuf, std::thread::JoinHandle<crate::app::ReadFile>)>> = const { RefCell::new(None) };
}

/// Opens a spreadsheet as vl_open, reading it on a thread of its own so the app can show
/// Opening… meanwhile: 1 while it reads (vl_open_poll tells when it's done), else as vl_open
/// (a template opens at once, as a new spreadsheet).
#[unsafe(no_mangle)]
pub extern "C" fn vl_open_start(path: *const c_char) -> i32 {
    let p = PathBuf::from(arg(path));
    if crate::app::is_template(&p) {
        return vl_open(path);
    }
    let q = p.clone();
    let reading = std::thread::spawn(move || crate::app::read_file(&q));
    OPENING.with(|o| *o.borrow_mut() = Some((p, reading)));
    1
}

/// The spreadsheet vl_open_start reads: 1 while it still reads, else as vl_open (it's the
/// document now, or the message is in vl_error).
#[unsafe(no_mangle)]
pub extern "C" fn vl_open_poll() -> i32 {
    let Some((p, reading)) = OPENING.with(|o| {
        let mut o = o.borrow_mut();
        if o.as_ref().is_some_and(|(_, r)| !r.is_finished()) { None } else { o.take() }
    }) else {
        return if OPENING.with(|o| o.borrow().is_some()) { 1 } else { -1 };
    };
    let read = match reading.join() {
        Ok(r) => r,
        Err(_) => {
            ERR.with(|e| *e.borrow_mut() = "it isn't a spreadsheet Values can read".into());
            return -1;
        }
    };
    with(|h| {
        crate::canvas::commit(&mut h.app);
        h.app.templates.chooser = false;
        let before = h.app.version;
        h.app.finish_open(&p, read);
        if h.app.version == before {
            ERR.with(|e| *e.borrow_mut() = h.app.message.as_ref().map(|m| m.0.clone()).unwrap_or_else(|| "it isn't a spreadsheet Values can read".into()));
            return if h.app.password.is_some() { 0 } else { -1 };
        }
        0
    })
}

/// Saves to `path`: .numbers, or .xlsx (Excel mode, as the desktop's Save of a workbook).
#[unsafe(no_mangle)]
pub extern "C" fn vl_save(path: *const c_char) -> i32 {
    let p = PathBuf::from(arg(path));
    with(|h| {
        crate::canvas::commit(&mut h.app);
        h.app.message = None;
        if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("xlsx")) {
            let compat = h.app.excel.as_ref().is_some_and(|x| x.compat);
            h.app.save_excel(&p, compat);
        } else {
            h.app.save_numbers_to(&p);
        }
        match h.app.message.as_ref().map(|m| m.0.clone()) {
            Some(m) if m.starts_with("Couldn't") => {
                ERR.with(|e| *e.borrow_mut() = m);
                -1
            }
            _ => 0,
        }
    })
}

/// Exports to `path` by its extension: pdf (the print setup), csv / tsv (the sheet's first table,
/// or the selected one).
#[unsafe(no_mangle)]
pub extern "C" fn vl_export(path: *const c_char) -> i32 {
    let p = PathBuf::from(arg(path));
    with(|h| {
        crate::canvas::commit(&mut h.app);
        let Some(wb) = h.app.wb.as_ref() else { return -1 };
        let ext = p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        let res: Result<(), String> = match ext.as_str() {
            "pdf" => {
                let by_table = h.app.canvas.tiles.extras.clone();
                crate::pdf::export_with(wb, &p, by_table).map(|_| ())
            }
            "csv" | "tsv" => {
                let sep = if ext == "csv" { ',' } else { '\t' };
                let table = h.app.sel.map(|s| s.table).or_else(|| h.app.drawables.iter().find(|d| matches!(d.kind, nm_core::kn_model::DrawableKind::Table(_))).map(|d| d.id));
                match table.and_then(|t| wb.table_data(t)) {
                    Some(t) => {
                        let field = |s: &str| if s.contains(sep) || s.contains('"') || s.contains('\n') { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() };
                        let text: String = (0..t.rows).map(|r| (0..t.cols).map(|c| field(t.cells.get((r * t.cols + c) as usize).map(String::as_str).unwrap_or(""))).collect::<Vec<_>>().join(&sep.to_string()) + "\n").collect();
                        std::fs::write(&p, text).map_err(|e| e.to_string())
                    }
                    None => Err("there's no table on this sheet".into()),
                }
            }
            _ => Err(format!("Values can't export .{ext}")),
        };
        match res {
            Ok(()) => 0,
            Err(e) => {
                ERR.with(|x| *x.borrow_mut() = e);
                -1
            }
        }
    })
}

// ---- the sheet ----

/// Where the view is: the sheet point at its top-left and the zoom, as the front end scrolls
/// and zooms it.
#[unsafe(no_mangle)]
pub extern "C" fn vl_scroll(x: f32, y: f32, zoom: f32) {
    with(|h| crate::api::set_scroll(&mut h.app, x, y, zoom))
}

/// {zoom, scroll (after the engine moved it: a cell kept in view, a sheet opened), content,
/// version, sheet, tile}.
#[unsafe(no_mangle)]
pub extern "C" fn vl_geometry() -> *mut c_char {
    json_out(with(|h| Some(crate::api::geometry(&h.app))))
}

/// Tile (`tx`, `ty`) of the sheet at `scale` pixels per point into `buf` (TILE × TILE, RGBA
/// premultiplied; the tile is TILE / scale sheet points square from (tx, ty) × that).
#[unsafe(no_mangle)]
pub extern "C" fn vl_render_tile(tx: i32, ty: i32, scale: f32, buf: *mut u8, cap: u32) -> i32 {
    with(|h| {
        let Some(pm) = crate::api::tile(&mut h.app, tx, ty, scale) else { return -1 };
        let data = pm.data();
        if buf.is_null() || data.len() > cap as usize {
            return -1;
        }
        unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), buf, data.len()) };
        0
    })
}

/// A table's frozen header rows (`cols` false) or its header columns over rows r0..=r1 at
/// `scale` pixels a point into `buf` (RGBA premultiplied, at most `cap` bytes); its size in
/// pixels into `w` / `h`. Non-zero when there's none or it doesn't fit.
#[unsafe(no_mangle)]
pub extern "C" fn vl_render_header(id: u64, cols: bool, r0: u32, r1: u32, scale: f32, buf: *mut u8, cap: u32, w: *mut u32, h: *mut u32) -> i32 {
    with(|host| {
        let Some((pm, _)) = crate::api::header(&mut host.app, id as nm_core::kn_format::ObjId, cols, r0, r1, scale) else { return -1 };
        let data = pm.data();
        if buf.is_null() || w.is_null() || h.is_null() || data.len() > cap as usize {
            return -1;
        }
        unsafe {
            std::ptr::copy_nonoverlapping(data.as_ptr(), buf, data.len());
            *w = pm.width();
            *h = pm.height();
        }
        0
    })
}

/// What's drawn over the sheet (api::overlays).
#[unsafe(no_mangle)]
pub extern "C" fn vl_overlays() -> *mut c_char {
    json_out(with(|h| Some(crate::api::overlays(&mut h.app))))
}

/// The selected table for its chrome (api::table_state), null if none.
#[unsafe(no_mangle)]
pub extern "C" fn vl_table() -> *mut c_char {
    json_out(with(|h| Some(crate::api::table_state(&mut h.app))))
}

/// A row (`rows`) or column tab's menu items.
#[unsafe(no_mangle)]
pub extern "C" fn vl_table_tab_menu(rows: bool, i: u32) -> *mut c_char {
    json_out(with(|h| Some(crate::api::tab_menu(&h.app, rows, i))))
}

/// The cell under view point (x, y): {table, r, c} or null.
#[unsafe(no_mangle)]
pub extern "C" fn vl_hit(x: f32, y: f32) -> *mut c_char {
    json_out(with(|h| Some(crate::api::hit(&h.app, pos2(x, y)))))
}

/// The pointer on the view (points from its top-left): kind 0 moved (held), 1 pressed, 2
/// released; button 1 the secondary (a right-click, a touch and hold); Shift held.
#[unsafe(no_mangle)]
pub extern "C" fn vl_pointer(kind: u32, x: f32, y: f32, button: u32, shift: bool, _ctrl: bool) {
    with(|h| {
        let t = h.start.elapsed().as_secs_f64();
        crate::api::pointer(&mut h.app, kind, pos2(x, y), button == 1, shift, t);
    })
}

/// The edit menu's items for what's selected now: [{label, action, enabled} | {separator}].
#[unsafe(no_mangle)]
pub extern "C" fn vl_context_menu() -> *mut c_char {
    json_out(with(|h| Some(crate::api::context_menu(&h.app))))
}

/// A toolbar popover's contents (api::popover).
#[unsafe(no_mangle)]
pub extern "C" fn vl_popover(id: *const c_char) -> *mut c_char {
    let id = arg(id);
    json_out(with(|h| Some(crate::api::popover(&h.app, &id))))
}

/// Runs a menu or popover item's action (api::action).
#[unsafe(no_mangle)]
pub extern "C" fn vl_action(action: *const c_char) {
    let a = arg(action);
    with(|h| {
        let ctx = h.ctx.clone();
        crate::api::action(&mut h.app, &ctx, &a);
    })
}

// ---- typing ----

/// Typed text (a keyboard), as typed on the desktop.
#[unsafe(no_mangle)]
pub extern "C" fn vl_text(text: *const c_char) {
    let t = arg(text);
    with(|h| {
        let ctx = h.ctx.clone();
        crate::api::text(&mut h.app, &ctx, &t);
    })
}

/// A key by egui's name (Backspace, Delete, Enter, Tab, ArrowLeft, Home…) with Shift, Ctrl and
/// Alt. Ctrl with a letter is a menu shortcut.
#[unsafe(no_mangle)]
pub extern "C" fn vl_key(name: *const c_char, shift: bool, ctrl: bool, alt: bool) {
    let name = arg(name);
    with(|h| {
        let ctx = h.ctx.clone();
        crate::api::key(&mut h.app, &ctx, &name, shift, ctrl, alt);
    })
}

/// Pasted text (the system clipboard), as Edit > Paste.
#[unsafe(no_mangle)]
pub extern "C" fn vl_paste(text: *const c_char) {
    let t = arg(text);
    with(|h| {
        let ctx = h.ctx.clone();
        crate::api::paste(&mut h.app, &ctx, &t);
    })
}

/// Text the last commands copied (Copy, Cut), once; null if none.
#[unsafe(no_mangle)]
pub extern "C" fn vl_copied() -> *mut c_char {
    match with(|h| h.copied.take()) {
        Some(s) => out(s),
        None => std::ptr::null_mut(),
    }
}

// ---- commands and state ----

/// The menu bar as data (api::menus).
#[unsafe(no_mangle)]
pub extern "C" fn vl_menus() -> *mut c_char {
    json_out(with(|h| Some(crate::api::menus(&h.app))))
}

/// Runs a menu command by its name (as vl_menus gives it).
#[unsafe(no_mangle)]
pub extern "C" fn vl_run(command: *const c_char) {
    let c = arg(command);
    with(|h| {
        let ctx = h.ctx.clone();
        if let Some(p) = c.strip_prefix("open:") {
            h.app.open(std::path::Path::new(p));
        } else {
            crate::menus::run(&mut h.app, &ctx, &c);
        }
    })
}

/// A toolbar item without a popover (Text, Comment, Format, Organize…), by its id.
#[unsafe(no_mangle)]
pub extern "C" fn vl_toolbar(id: *const c_char) {
    let id = arg(id);
    with(|h| {
        let ctx = h.ctx.clone();
        crate::api::toolbar_press(&mut h.app, &ctx, &id);
    })
}

/// The inspector as data: which tabs show and their controls' values (inspapi::state).
#[unsafe(no_mangle)]
pub extern "C" fn vl_inspector() -> *mut c_char {
    json_out(with(|h| Some(crate::inspapi::state(&mut h.app))))
}

/// Changes an inspector control: `key` as vl_inspector names it, `value` JSON; "dlg.…" a
/// window's (dlgapi).
#[unsafe(no_mangle)]
pub extern "C" fn vl_inspector_set(key: *const c_char, value: *const c_char) {
    let (k, v) = (arg(key), arg(value));
    let v: Value = serde_json::from_str(&v).unwrap_or(Value::Null);
    with(|h| {
        let ctx = h.ctx.clone();
        match k.strip_prefix("dlg.") {
            Some(k) => crate::dlgapi::set(&mut h.app, &ctx, k, &v),
            None => crate::inspapi::set(&mut h.app, &ctx, &k, &v),
        }
    })
}

/// The document's state: title, edited, the message shown, undo / redo, the inspector, the
/// sheets, the cell bar, what's being edited, the windows open.
#[unsafe(no_mangle)]
pub extern "C" fn vl_status() -> *mut c_char {
    json_out(with(|h| {
        let a = &mut h.app;
        let cell_bar = crate::api::cell_bar(a);
        // background work coming back (movies, web videos, stock quotes); the media browser's list
        let busy = crate::media::jobs(a) | a.stocks.busy() | a.media.recorder.is_some();
        crate::stocks::poll(a);
        if a.media.open.is_some() && !BROWSER_LISTED.with(|b| b.replace(true)) {
            crate::dlgapi::list_browser(a);
        } else if a.media.open.is_none() {
            BROWSER_LISTED.with(|b| b.set(false));
        }
        let a = &h.app;
        Some(json!({
            "title": a.title(),
            "edited": a.doc().is_some_and(|d| d.dirty),
            "message": a.message.as_ref().map(|m| m.0.clone()),
            "canUndo": a.hist.can_undo() || a.canvas.edit.is_some(),
            "canRedo": a.hist.can_redo(),
            "inspector": a.inspector,
            "editingText": a.canvas.editing.is_some(),
            "editingCell": a.canvas.edit.is_some(),
            "objects": a.selected_objs().len(),
            "cells": a.sel.is_some(),
            "sheets": crate::api::sheets(a),
            "cellBar": cell_bar,
            "find": crate::api::find_state(a),
            "excel": a.excel.is_some(),
            "windows": crate::dlgapi::state(a),
            "savedColors": a.settings.saved_colors,
            "openUrls": URLS.lock().ok().and_then(|mut q| q.as_mut().map(std::mem::take)).unwrap_or_default(),
            "busy": busy,
            // what to say ("" stops speaking)
            "speak": SPEECH.lock().ok().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default(),
        }))
    }))
}

/// The window's chrome as data, for the front end to draw itself: the look's colours (the
/// desktop's palette, so both draw alike), the title and the toolbar's items and states.
#[unsafe(no_mangle)]
pub extern "C" fn vl_chrome() -> *mut c_char {
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
        let sub = match (a.excel.as_ref(), dirty) {
            (Some(_), true) => Some("Excel · Edited".to_string()),
            (Some(_), false) => Some("Excel".to_string()),
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
                "tabs": c(crate::look::tabs_bg()),
                "canvas": c(egui::Color32::WHITE),
                "capsule": c(crate::look::capsule_fill()),
                "styleButton": c(theme::CONTROL_BG()),
                "styleButtonText": c(theme::TEXT()),
                "controlStroke": c(if theme::is_light() { theme::tint(egui::Color32::from_rgb(0xcc, 0xcc, 0xcc)) } else { egui::Color32::TRANSPARENT }),
                "gray": grays,
                "titleBar": c(vis.widgets.open.weak_bg_fill),
                "hover": c(vis.widgets.hovered.weak_bg_fill),
                "windowStroke": c(vis.window_stroke.color),
                "field": c(vis.text_edit_bg_color()),
                "fieldStroke": c(vis.widgets.inactive.bg_stroke.color),
                "weakText": c(vis.weak_text_color()),
                "glassTint": c(theme::tint(if theme::is_light() { egui::Color32::from_rgb(0xfa, 0xfa, 0xfa) } else { egui::Color32::from_rgb(0x2a, 0x2a, 0x2c) })),
                "toolbarTint": c(crate::look::header_bg()),
                "tabBar": c(crate::look::tabs_bg()),
                "tabActive": c(if theme::is_light() { theme::tint(egui::Color32::WHITE) } else { theme::tint(egui::Color32::from_rgb(0x4a, 0x4a, 0x4a)) }),
                "tabLine": c(if theme::is_light() { theme::tint(egui::Color32::from_gray(0xc8)) } else { theme::tint(egui::Color32::from_gray(0x48)) }),
            },
            "title": a.title(),
            "sub": sub,
            "zoom": a.canvas.zoom,
            "showToolbar": a.show_toolbar,
            "glass": a.look.liquid_glass,
            "translucent": a.look.translucent_panes,
            "classic": a.look.classic_look,
            // (Values' toolbar sits on the header's own colour: nothing of the sheet under it)
            "toolbarBackground": "solid",
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

/// A web page asked for (Values Help, Support Development…): the front end opens it.
/// Text to speak (Edit > Speech; None: Stop Speaking), for the front end's speech.
static SPEECH: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Start Speaking (`Some`) / Stop Speaking (`None`) for a front end that speaks.
pub fn speak(text: Option<&str>) {
    if let Ok(mut q) = SPEECH.lock() {
        q.push(text.unwrap_or_default().to_string());
    }
}

/// Whether the front end opens the links (it asked for them in vl_init).
pub fn front_end_opens_urls() -> bool {
    URLS.lock().ok().is_some_and(|q| q.is_some())
}

pub fn open_url(url: &str) {
    match URLS.lock().ok().as_mut().and_then(|q| q.as_mut()) {
        Some(q) => q.push(url.to_string()),
        None => kn_ui::open_url(url),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(p: *mut c_char) -> Value {
        let v = serde_json::from_str(&unsafe { CStr::from_ptr(p) }.to_string_lossy()).unwrap_or(Value::Null);
        vl_free_str(p);
        v
    }

    fn s(t: &str) -> CString {
        CString::new(t).unwrap()
    }

    /// A tap on a cell, typing over it, Return; a formula; the edit menu; a tile drawn; a save
    /// and reopen: all without any egui pass.
    #[test]
    fn spreadsheet_api() {
        let home = std::env::temp_dir().join("values-ffi-test");
        let _ = std::fs::remove_dir_all(&home);
        vl_home(s(home.to_string_lossy().as_ref()).as_ptr());
        vl_init(800.0, 1000.0);
        let g = json(vl_geometry());
        assert!(g["zoom"].as_f64().unwrap() > 0.1, "{g}");
        // the blank spreadsheet's table: tap its second body cell (B2)
        let t = {
            let (zoom, sx, sy) = (g["zoom"].as_f64().unwrap() as f32, g["scroll"][0].as_f64().unwrap() as f32, g["scroll"][1].as_f64().unwrap() as f32);
            let c = g["content"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap() as f32).collect::<Vec<_>>();
            // a point inside the content near its top-left, well into the second row and column
            ((c[0] + 140.0 - sx) * zoom, (c[1] + 30.0 - sy) * zoom)
        };
        vl_pointer(1, t.0, t.1, 0, false, false);
        vl_pointer(2, t.0, t.1, 0, false, false);
        let tb = json(vl_table());
        assert!(tb["sel"].is_array(), "a cell selected: {tb}");
        vl_text(s("42").as_ptr());
        let tb = json(vl_table());
        assert_eq!(tb["editing"]["text"], "42", "{tb}");
        vl_action(s("tbl:commitDown").as_ptr());
        let st = json(vl_status());
        assert_eq!(st["canUndo"], true, "{st}");
        // a formula in the cell below
        vl_text(s("=").as_ptr());
        vl_action(s("tbl:text:=B2*2").as_ptr());
        vl_action(s("tbl:commit").as_ptr());
        let st = json(vl_status());
        assert!(st["cellBar"]["formula"].as_str().is_some_and(|f| f.contains("B2")), "{st}");
        let m = json(vl_context_menu());
        assert!(m.as_array().unwrap().iter().any(|i| i["label"] == "Copy"), "{m}");
        vl_run(s("Copy").as_ptr());
        let copied = vl_copied();
        assert!(!copied.is_null());
        vl_free_str(copied);
        let mut buf = vec![0u8; (crate::tiles::TILE * crate::tiles::TILE * 4) as usize];
        assert_eq!(vl_render_tile(0, 0, 2.0, buf.as_mut_ptr(), buf.len() as u32), 0);
        assert!(buf.chunks(4).any(|p| p[3] > 0 && p[0] < 200), "something drawn in the first tile");
        let ch = json(vl_chrome());
        assert!(ch["toolbar"].as_array().unwrap().len() > 5);
        let menus = json(vl_menus());
        assert!(menus.as_array().unwrap().len() > 5);
        // save and open again
        let file = home.join("t.numbers");
        assert_eq!(vl_save(s(file.to_string_lossy().as_ref()).as_ptr()), 0);
        assert_eq!(vl_open(s(file.to_string_lossy().as_ref()).as_ptr()), 0);
        let xl = home.join("t.xlsx");
        assert_eq!(vl_save(s(xl.to_string_lossy().as_ref()).as_ptr()), 0);
        assert!(xl.exists());
        assert_eq!(vl_open(s(xl.to_string_lossy().as_ref()).as_ptr()), 0);
        let st = json(vl_status());
        assert_eq!(st["excel"], true, "{st}");
    }

    #[test]
    fn canvas_controls_and_windows() {
        let home = std::env::temp_dir().join("values-ffi-test-2");
        let _ = std::fs::remove_dir_all(&home);
        vl_home(s(home.to_string_lossy().as_ref()).as_ptr());
        vl_init(800.0, 1000.0);
        let ov = |k: &str| json(vl_overlays())["controls"][k].clone();
        // a shape's points
        vl_run(s("Shape > Rectangle").as_ptr());
        vl_run(s("Shapes and Lines > Make Editable").as_ptr());
        let p = ov("points");
        assert!(p["nodes"].as_array().is_some_and(|n| n.len() >= 4), "points: {p}");
        vl_action(s("ctl:points:move:0:0:0:5:5").as_ptr());
        assert!(json(vl_status())["canUndo"] == true);
        // a guide out of the top ruler, then removed
        vl_run(s("Show Rulers").as_ptr());
        vl_action(s("ctl:guide::h:120").as_ptr());
        assert_eq!(ov("guides")["lines"].as_array().map(|l| l.len()), Some(1), "{}", ov("guides"));
        vl_action(s("ctl:guide:0:h:x").as_ptr());
        assert_eq!(ov("guides")["lines"].as_array().map(|l| l.len()), Some(0));
        // the pen: a stroke becomes a shape
        vl_run(s("Line > Draw With Pen").as_ptr());
        assert!(ov("pen").is_object());
        let before = json(vl_overlays())["version"].clone();
        vl_action(s("ctl:pen:line:10,10;40,30;80,20").as_ptr());
        assert_ne!(json(vl_overlays())["version"], before);
        vl_action(s("ctl:pen:done").as_ptr());
        assert!(ov("pen").is_null());
        // Values Help and speech go to the front end
        vl_run(s("Values Help").as_ptr());
        let st = json(vl_status());
        assert!(st["openUrls"].as_array().is_some_and(|a| a.iter().any(|u| u.as_str().is_some_and(|u| u.ends_with("help.html")))), "{}", st["openUrls"]);
        // windows: Stock Quote, Language & Region, Show Warnings
        for (cmd, id) in [("Show Warnings", "warnings"), ("Save as Template…", "saveTemplate"), ("Web Video…", "webVideo"), ("Record Audio", "record")] {
            vl_run(s(cmd).as_ptr());
            let w = json(vl_status())["windows"].clone();
            assert!(w.as_array().is_some_and(|a| a.iter().any(|x| x["id"] == id)), "{cmd}: {w}");
        }
    }


    /// Every inspector control changes the document: the undo history grows (one edit made) and,
    /// where the inspector shows the value, it reads back as set. Failures are listed together.
    #[test]
    fn every_control_changes_the_document() {
        let home = std::env::temp_dir().join("values-ffi-test-3");
        let _ = std::fs::remove_dir_all(&home);
        vl_home(s(home.to_string_lossy().as_ref()).as_ptr());
        vl_init(800.0, 1000.0);
        let undos = || with(|h| h.app.hist.undo_len());
        let insp = || json(vl_inspector());
        let set = |k: &str, v: Value| vl_inspector_set(s(k).as_ptr(), s(&v.to_string()).as_ptr());
        let failed: std::cell::RefCell<Vec<String>> = Default::default();
        // set `k` to `v`: an edit is made, and `path` in the pane's data (if given) reads `want`
        let check = |k: &str, v: Value, path: &str, want: Value| {
            let before = undos();
            set(k, v.clone());
            let after = undos();
            // (an edit with the same name as the one before merges into it: the value read back
            // decides then)
            if after <= before && path.is_empty() {
                failed.borrow_mut().push(format!("{k} = {v}: no edit"));
                return;
            }
            if !path.is_empty() {
                let st = insp();
                let mut got = &st["data"];
                for part in path.split('.') {
                    got = match part.parse::<usize>() {
                        Ok(i) => &got[i],
                        Err(_) => &got[part],
                    };
                }
                let ok = match (got.as_f64(), want.as_f64()) {
                    (Some(a), Some(b)) => (a - b).abs() < 0.01,
                    _ => *got == want,
                };
                if !ok {
                    failed.borrow_mut().push(format!("{k} = {v}: {path} reads {got}, not {want}"));
                }
            }
        };
        // ---- the blank spreadsheet's table, cell B2 (tapped)
        let g = json(vl_geometry());
        let (zoom, sx, sy) = (g["zoom"].as_f64().unwrap() as f32, g["scroll"][0].as_f64().unwrap() as f32, g["scroll"][1].as_f64().unwrap() as f32);
        let c = g["content"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap() as f32).collect::<Vec<_>>();
        let t = ((c[0] + 140.0 - sx) * zoom, (c[1] + 30.0 - sy) * zoom);
        vl_pointer(1, t.0, t.1, 0, false, false);
        vl_pointer(2, t.0, t.1, 0, false, false);
        assert!(json(vl_table())["sel"].is_array());
        set("obj.cellTab", json!(0));
        check("table.headerRows", json!(2), "headerRows", json!(2));
        check("table.headerCols", json!(0), "headerCols", json!(0));
        check("table.footerRows", json!(1), "footerRows", json!(1));
        check("table.title", json!(false), "title", json!(false));
        check("table.caption", json!(true), "caption", json!(true));
        check("table.rows", json!(25), "rows", json!(25));
        check("table.cols", json!(6), "cols", json!(6));
        check("table.banded", json!(true), "banded", json!(true));
        check("table.bandColor", json!([1.0, 0.0, 0.0, 1.0]), "", Value::Null);
        let fs = insp()["data"]["fontSize"].as_f64().unwrap_or(0.0);
        check("table.fontBigger", json!(true), "fontSize", json!(fs + 1.0));
        check("table.outline", json!({"color": [1.0, 0.0, 0.0, 1.0], "width": 2.0}), "outline.width", json!(2.0));
        let g0 = insp()["data"]["gridlines"][0].as_bool().unwrap_or(false);
        check("table.gridline", json!(0), "gridlines.0", json!(!g0));
        check("table.height", json!(40), "height", json!(40));
        check("table.width", json!(90), "width", json!(90));
        let preset = insp()["data"]["presets"][2]["id"].clone();
        check("table.preset", preset, "", Value::Null);
        set("obj.cellTab", json!(1));
        check("cell.format", json!({"kind": 3, "decimals": 1}), "format.kind", json!(3));
        check("cell.fill", json!({"kind": 1, "color": [0.0, 1.0, 0.0, 1.0]}), "fill.kind", json!(1));
        set("cell.edges", json!(0));
        check("cell.line", json!({"color": [0.0, 0.0, 1.0, 1.0], "width": 2.0}), "", Value::Null);
        check("cell.condAdd", json!({"cmp": 2, "value": "1", "look": 0}), "cond.rules.0.look", insp()["data"]["cond"]["looks"][0]["name"].clone());
        check("cell.condRemove", json!(0), "cond.rules.0", Value::Null);
        set("obj.cellTab", json!(2));
        check("celltext.size", json!(18), "size", json!(18));
        check("celltext.bold", json!(true), "bold", json!(true));
        check("celltext.italic", json!(true), "italic", json!(true));
        check("celltext.underline", json!(true), "underline", json!(true));
        check("celltext.strike", json!(true), "strike", json!(true));
        check("celltext.color", json!([1.0, 0.0, 0.0, 1.0]), "color.0", json!(1.0));
        check("celltext.align", json!(1), "align", json!(1));
        check("celltext.valign", json!(1), "valign", json!(1));
        check("celltext.wrap", json!(false), "wrap", json!(false));
        check("celltext.inset", json!(6), "inset", json!(6));
        check("celltext.first", json!(5), "first", json!(5));
        check("celltext.left", json!(4), "left", json!(4));
        check("celltext.right", json!(3), "right", json!(3));
        check("celltext.lines", json!(1.5), "lines", json!(1.5));
        check("celltext.before", json!(2), "before", json!(2));
        check("celltext.after", json!(2), "after", json!(2));
        // ---- the sheet
        vl_run(s("Deselect All").as_ptr());
        with(|h| {
            h.app.sel = None;
            h.app.canvas.obj = None;
        });
        check("sheet.name", json!("Budget"), "", Value::Null);
        let sheet_st = insp();
        if sheet_st["name"] != "Budget" {
            failed.borrow_mut().push(format!("sheet.name: reads {}", sheet_st["name"]));
        }
        // (the sheet pane's values are at its top level)
        check("sheet.tabColor", json!([1.0, 0.0, 0.0, 1.0]), "", Value::Null);
        if insp()["tabColor"].is_null() {
            failed.borrow_mut().push("sheet.tabColor: reads null".into());
        }
        // ---- a text box: Style, Text, Arrange
        vl_run(s("Text").as_ptr());
        with(|h| h.app.canvas.editing = None);
        set("obj.tab", json!(0));
        check("style.fill", json!({"kind": 1, "color": [1.0, 0.0, 0.0, 1.0]}), "fill.kind", json!(1));
        check("style.stroke", json!({"color": [0.0, 0.0, 1.0, 1.0], "width": 3.0}), "stroke.width", json!(3.0));
        check("style.shadowKind", json!(1), "shadow.kind", json!(0));
        check("style.opacity", json!(50), "opacity", json!(0.5));
        check("style.reflection", json!(true), "reflection", json!(0.5));
        check("style.title", json!(true), "title", json!(true));
        check("style.caption", json!(true), "caption", json!(true));
        set("obj.tab", json!(1));
        let tab = insp()["kind"].clone();
        if tab != "Text" {
            failed.borrow_mut().push(format!("text box tab 1 is {tab}"));
        }
        check("objtext.size", json!(20), "size", json!(20));
        check("objtext.typeface", json!(3), "bold", json!(true));
        check("objtext.underline", json!(true), "underline", json!(true));
        check("objtext.color", json!([0.0, 0.5, 0.0, 1.0]), "color.1", json!(0.5));
        check("objtext.tracking", json!(10), "tracking", json!(10));
        check("objtext.script", json!(1), "script", json!(1));
        check("objtext.caps", json!("All"), "caps", json!("All"));
        check("objtext.align", json!(2), "align", json!(2));
        check("objtext.valign", json!(2), "valign", json!(2));
        check("objtext.lines", json!(1.5), "lines", json!(1.5));
        check("objtext.before", json!(4), "before", json!(4));
        check("objtext.after", json!(4), "after", json!(4));
        check("objtext.bullets", json!(true), "bullets", json!(true));
        check("objtext.inset", json!(8), "inset", json!(8));
        check("objtext.shrink", json!(true), "shrink", json!(true));
        check("objtext.columns", json!(2), "columns", json!(2));
        check("objtext.gutter", json!(20), "gutter", json!(20));
        let para = insp()["data"]["paraStyles"].as_array().and_then(|a| a.iter().find(|p| p["name"] != insp()["data"]["paraStyle"]).cloned());
        if let Some(p) = para {
            check("objtext.paraStyle", p["id"].clone(), "paraStyle", p["name"].clone());
        } else {
            failed.borrow_mut().push("objtext: no other paragraph style".into());
        }
        set("obj.tab", json!(2));
        check("arrange.geometry", json!({"w": 150.0}), "", Value::Null);
        check("arrange.geometry", json!({"angle": 30.0}), "", Value::Null);
        check("arrange.command", json!("Flip Horizontally"), "", Value::Null);
        check("arrange.command", json!("Lock"), "", Value::Null);
        check("arrange.command", json!("Unlock"), "", Value::Null);
        // ---- a chart
        vl_run(s("Chart > 2D Column").as_ptr());
        set("obj.tab", json!(0));
        check("chart.legend", json!(false), "legend", json!(false));
        check("chart.title", json!(true), "title", json!("Title"));
        check("chart.border", json!(true), "border", json!(true));
        check("chart.barGap", json!(30), "barGap", json!(30));
        let types = insp()["data"]["types"].clone();
        let other = types.as_array().and_then(|a| a.iter().map(|t| t[0].clone()).find(|t| *t != insp()["data"]["type"])).unwrap_or(Value::Null);
        check("chart.type", other.clone(), "type", other);
        let tabs = insp()["tabs"].clone();
        if let Some(i) = tabs.as_array().and_then(|a| a.iter().position(|t| t == "Series")) {
            set("obj.tab", json!(i));
            check("chart.valueLabels", json!(true), "valueLabels", json!(true));
        }
        if let Some(i) = tabs.as_array().and_then(|a| a.iter().position(|t| t == "Axis")) {
            set("obj.tab", json!(i));
            check("chart.gridlines", json!(false), "gridlines", json!(false));
            check("chart.min", json!(5), "min", json!(5));
        }
        let failed = failed.into_inner();
        assert!(failed.is_empty(), "controls that don't change the document:\n{}", failed.join("\n"));
    }


    /// The windows' controls and the menu commands that edit change the document (the undo
    /// history grows, or the value reads back). Failures are listed together.
    #[test]
    fn windows_and_menus_change_the_document() {
        let home = std::env::temp_dir().join("values-ffi-test-4");
        let _ = std::fs::remove_dir_all(&home);
        vl_home(s(home.to_string_lossy().as_ref()).as_ptr());
        vl_init(800.0, 1000.0);
        let undos = || with(|h| h.app.hist.undo_len());
        let set = |k: &str, v: Value| vl_inspector_set(s(k).as_ptr(), s(&v.to_string()).as_ptr());
        let failed: std::cell::RefCell<Vec<String>> = Default::default();
        // the Shape popover's Browse Content Hub opens the hub on Shapes (Insert's on Browse)
        vl_action(s("media:Content Hub Shapes").as_ptr());
        assert_eq!(with(|h| h.app.hub.tab_index()), 4);
        vl_action(s("media:Content Hub").as_ptr());
        assert_eq!(with(|h| h.app.hub.tab_index()), 0);
        with(|h| h.app.hub.open = false);
        let tap_b2 = || {
            let g = json(vl_geometry());
            let (zoom, sx, sy) = (g["zoom"].as_f64().unwrap() as f32, g["scroll"][0].as_f64().unwrap() as f32, g["scroll"][1].as_f64().unwrap() as f32);
            let c = g["content"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap() as f32).collect::<Vec<_>>();
            let t = ((c[0] + 140.0 - sx) * zoom, (c[1] + 30.0 - sy) * zoom);
            vl_pointer(1, t.0, t.1, 0, false, false);
            vl_pointer(2, t.0, t.1, 0, false, false);
        };
        tap_b2();
        // values in B2:B4 (the formats need values; sorting and summing them)
        for v in ["7", "3", "5"] {
            vl_text(s(v).as_ptr());
            vl_action(s("tbl:commitDown").as_ptr());
        }
        tap_b2();
        vl_action(s("tbl:cancel").as_ptr());
        // menu commands on the selected cell, each a new edit
        for cmd in [
            "Table > Add Row Above", "Table > Add Column After", "Table > Insert Current Date", "Font > Bold", "Text > Align Center",
            "Sort Ascending", "Sort Descending", "Table > Freeze Header Rows", "Table > Hide Column", "Table > Unhide All Columns", "Header Rows > 2",
            "Table > Delete Row", "Clear All",
        ] {
            let b = undos();
            vl_run(s(cmd).as_ptr());
            vl_action(s("tbl:commit").as_ptr());
            if undos() <= b {
                failed.borrow_mut().push(format!("menu {cmd}: no edit"));
            }
            tap_b2();
            vl_action(s("tbl:cancel").as_ptr());
        }
        // Formula > Sum: the formula started in the cell, finished by typing
        vl_run(s("Formula > Sum").as_ptr());
        if !json(vl_table())["editing"]["text"].as_str().is_some_and(|t| t.starts_with("=SUM(")) {
            failed.borrow_mut().push("menu Formula > Sum: no =SUM( started".into());
        }
        vl_action(s("tbl:cancel").as_ptr());
        vl_text(s("8").as_ptr());
        vl_action(s("tbl:commit").as_ptr());
        // windows: Fonts (size), Colors (Fill Selection), Language & Region, Print Setup (paper)
        vl_run(s("Font > Show Fonts").as_ptr());
        let w = |name: &str, k: &str, v: Value| {
            let b = undos();
            set(k, v);
            if undos() <= b {
                failed.borrow_mut().push(format!("{name}: {k} made no edit"));
            }
        };
        w("Fonts", "dlg.fonts.size", json!(23));
        w("Fonts", "dlg.fonts.family", json!("DejaVu Sans"));
        w("Fonts", "dlg.fonts.color", json!([0.2, 0.4, 0.6, 1.0]));
        vl_run(s("Show Colors").as_ptr());
        w("Colors", "dlg.colors.apply", json!([255, 0, 0, 255]));
        vl_run(s("Advanced > Language & Region…").as_ptr());
        w("Language", "dlg.language.ok", json!({"lang": "fr", "region": "FR"}));
        vl_run(s("Print…").as_ptr());
        w("Print Setup", "dlg.printSetup.portrait", json!(false));
        let pn = json(vl_status())["windows"].as_array().and_then(|a| a.iter().find(|x| x["id"] == "printSetup").map(|x| x["pageNumbers"].as_bool().unwrap_or(false))).unwrap_or(false);
        set("dlg.printSetup.pageNumbers", json!(!pn));
        let pn2 = json(vl_status())["windows"].as_array().and_then(|a| a.iter().find(|x| x["id"] == "printSetup").map(|x| x["pageNumbers"].as_bool().unwrap_or(false))).unwrap_or(false);
        if pn2 == pn {
            failed.borrow_mut().push("Print Setup: pageNumbers didn't change".into());
        }
        // the sheet tabs' and sheet pane's buttons
        let n = || json(vl_status())["sheets"]["sheets"].as_array().map(|a| a.len()).unwrap_or(0);
        let before = n();
        vl_action(s("sheet:add").as_ptr());
        if n() != before + 1 {
            failed.borrow_mut().push("sheet:add made no sheet".into());
        }
        // an object's link (the link popover) and its badge
        vl_run(s("Shape > Rectangle").as_ptr());
        vl_run(s("Add Link > Webpage").as_ptr());
        set("dlg.valuesLink.done", json!({"addr": "example.com"}));
        if json(vl_overlays())["controls"]["links"].as_array().is_none_or(|a| a.is_empty()) {
            failed.borrow_mut().push("link popover: no link on the shape".into());
        }
        // a chart's data window
        vl_run(s("Chart > 2D Column").as_ptr());
        vl_run(s("Chart > Edit Chart Data…").as_ptr());
        let st = json(vl_status());
        if st["windows"].as_array().is_some_and(|a| a.iter().any(|x| x["id"] == "chartData")) {
            w("Chart Data", "dlg.chartData.cell", json!({"r": 0, "c": 0, "text": "99"}));
            w("Chart Data", "dlg.chartData.addSeries", json!(null));
        }
        let failed = failed.into_inner();
        assert!(failed.is_empty(), "controls that don't change the document:\n{}", failed.join("\n"));
    }


    /// Every "cmd:" a front-end menu or popover sends (the cells', an object's and text's context
    /// menus, a row's and a column's menu, the toolbar popovers) is a command the engine runs:
    /// one of the menu bar's (implemented), or one menus::run handles itself.
    #[test]
    fn front_end_menus_send_known_commands() {
        let home = std::env::temp_dir().join("values-ffi-test-5");
        let _ = std::fs::remove_dir_all(&home);
        vl_home(s(home.to_string_lossy().as_ref()).as_ptr());
        vl_init(800.0, 1000.0);
        let mut cmds: Vec<String> = Vec::new();
        fn walk(v: &Value, out: &mut Vec<String>) {
            match v {
                Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
                Value::Object(o) => {
                    if let Some(c) = o.get("action").and_then(Value::as_str).and_then(|a| a.strip_prefix("cmd:")) {
                        out.push(c.to_string());
                    }
                    o.values().for_each(|x| walk(x, out));
                }
                _ => {}
            }
        }
        let g = json(vl_geometry());
        let (zoom, sx, sy) = (g["zoom"].as_f64().unwrap() as f32, g["scroll"][0].as_f64().unwrap() as f32, g["scroll"][1].as_f64().unwrap() as f32);
        let c = g["content"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap() as f32).collect::<Vec<_>>();
        let t = ((c[0] + 140.0 - sx) * zoom, (c[1] + 30.0 - sy) * zoom);
        vl_pointer(1, t.0, t.1, 0, false, false);
        vl_pointer(2, t.0, t.1, 0, false, false);
        walk(&json(vl_context_menu()), &mut cmds);
        walk(&json(vl_table_tab_menu(true, 1)), &mut cmds);
        walk(&json(vl_table_tab_menu(false, 1)), &mut cmds);
        for p in ["View", "Insert", "Media", "Table", "Shape", "Chart"] {
            walk(&json(vl_popover(s(p).as_ptr())), &mut cmds);
        }
        vl_run(s("Shape > Rectangle").as_ptr());
        walk(&json(vl_context_menu()), &mut cmds);
        vl_run(s("Text").as_ptr());
        walk(&json(vl_context_menu()), &mut cmds);
        cmds.sort();
        cmds.dedup();
        // (handled by menus::run before the menu bar's own: not items of the menu bar)
        let own = ["Sort Ascending", "Sort Descending", "Comment", "Text", "Sheet", "Image Gallery", "Choose File"];
        let unknown: Vec<&String> = cmds.iter().filter(|c| !(crate::menus::is_command(c) || own.contains(&c.as_str()))).collect();
        assert!(unknown.is_empty(), "commands the menu bar doesn't have: {unknown:?}");
    }


    /// An empty cell copied and pasted over a filled one with text leaves it as the copy:
    /// no text and no fill.
    #[test]
    fn pasting_an_empty_cell_clears_fill_and_text() {
        let home = std::env::temp_dir().join("values-ffi-test-paste");
        let _ = std::fs::remove_dir_all(&home);
        vl_home(s(home.to_string_lossy().as_ref()).as_ptr());
        vl_init(800.0, 1000.0);
        let g = json(vl_geometry());
        let (zoom, sx, sy) = (g["zoom"].as_f64().unwrap() as f32, g["scroll"][0].as_f64().unwrap() as f32, g["scroll"][1].as_f64().unwrap() as f32);
        let c = g["content"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap() as f32).collect::<Vec<_>>();
        let t = ((c[0] + 140.0 - sx) * zoom, (c[1] + 30.0 - sy) * zoom);
        vl_pointer(1, t.0, t.1, 0, false, false);
        vl_pointer(2, t.0, t.1, 0, false, false);
        let set = |k: &str, v: Value| vl_inspector_set(s(k).as_ptr(), s(&v.to_string()).as_ptr());
        // B2: text and a green fill
        vl_action(s("tbl:select:1:1:1:1").as_ptr());
        vl_text(s("hello").as_ptr());
        vl_action(s("tbl:commit").as_ptr());
        vl_action(s("tbl:select:1:1:1:1").as_ptr());
        set("obj.cellTab", json!(1));
        set("cell.fill", json!({"kind": 1, "color": [0.0, 1.0, 0.0, 1.0]}));
        let cell = || {
            with(|h| {
                let id = h.app.sel.unwrap().table;
                h.app.table(id).map(|(_, d)| {
                    let i = (d.cols + 1) as usize;
                    (d.cells.get(i).cloned().unwrap_or_default(), d.cell_styles.get(i).map(|st| st.fill.clone()))
                })
            })
            .unwrap()
        };
        let (text, fill) = cell();
        assert_eq!(text, "hello");
        assert!(fill.is_some_and(|f| !matches!(f, nm_core::kn_model::Fill::None)), "B2 filled first");
        // copy an empty, unfilled cell (D5) and paste it on B2
        vl_action(s("tbl:select:4:3:4:3").as_ptr());
        vl_run(s("Copy").as_ptr());
        let copied = vl_copied();
        let text = if copied.is_null() { String::new() } else { unsafe { std::ffi::CStr::from_ptr(copied) }.to_string_lossy().into_owned() };
        // (the system clipboard gives no paste for empty text)
        assert!(!text.is_empty(), "an empty cell copies as something the clipboard pastes");
        vl_action(s("tbl:select:1:1:1:1").as_ptr());
        vl_paste(s(&text).as_ptr());
        let (text, fill) = cell();
        assert_eq!(text, "", "the text went");
        assert!(fill.as_ref().is_none_or(|f| matches!(f, nm_core::kn_model::Fill::None)), "the fill went: {fill:?}");
    }
}

