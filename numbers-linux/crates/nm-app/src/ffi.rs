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
    for (k, sub) in [("HOME", ""), ("XDG_CONFIG_HOME", "config"), ("XDG_DATA_HOME", "data"), ("XDG_CACHE_HOME", "cache")] {
        let d = if sub.is_empty() { home.clone() } else { home.join(sub) };
        let _ = std::fs::create_dir_all(&d);
        // (before the app starts any thread)
        unsafe { std::env::set_var(k, d) };
    }
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
            "busy": false,
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
}

