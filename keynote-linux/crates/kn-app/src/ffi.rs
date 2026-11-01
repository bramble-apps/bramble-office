//! Keydeck as a document library for other front ends (the Flutter app for Android): the
//! desktop's presentations, editing logic and slide drawing, with no interface of its own. The
//! front end draws everything the user sees and touches; it calls in with what happened (a tap
//! or a drag at a slide point, text typed, a command) and asks for state as data (the slides,
//! the selection and caret, the chrome, menus, the inspector) and for the slides' content as
//! pixels. The egui context kept here is only what the desktop's logic is written against
//! (clipboard output, key types): no pass of egui ever runs. Strings are UTF-8, answers JSON;
//! strings returned are freed with `kd_free_str`.

use std::cell::RefCell;
use std::ffi::{CStr, CString, c_char};
use std::path::PathBuf;

use egui::Event;
use kn_model::Point;
use serde_json::{Value, json};

use crate::app::App;

struct Host {
    ctx: egui::Context,
    app: App,
    /// Text the last command copied (Copy, Cut), for the system clipboard.
    copied: Option<String>,
    /// The slideshow: (slide, build step).
    show: Option<(usize, usize)>,
    /// A tab with unsaved changes asked to close: the front end asks what to do.
    unsaved: Option<usize>,
}

thread_local! {
    static HOST: RefCell<Option<Host>> = const { RefCell::new(None) };
    static ERR: RefCell<String> = const { RefCell::new(String::new()) };
}

/// Runs `f` on the app, then its housekeeping; a panic is caught (and logged), not carried into
/// the front end.
fn with<T: Default>(f: impl FnOnce(&mut Host) -> T) -> T {
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        HOST.with(|h| match h.try_borrow_mut() {
            Ok(mut g) => match g.as_mut() {
                Some(h) => {
                    let r = f(h);
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
                    // a slideshow started by a command (Play, Rehearse, Play from This Slide)
                    if let Some(p) = h.app.play.take() {
                        h.show = Some((p.index, 0));
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

fn fail(e: impl Into<String>) -> i32 {
    ERR.with(|x| *x.borrow_mut() = e.into());
    -1
}

/// A pixmap into the front end's buffer (`w` × `h` RGBA premultiplied).
fn copy_out(pm: &kn_render::tiny_skia::Pixmap, buf: *mut u8, w: u32, h: u32) -> i32 {
    if buf.is_null() {
        return -1;
    }
    let out = unsafe { std::slice::from_raw_parts_mut(buf, (w * h * 4) as usize) };
    let (pw, ph) = (pm.width().min(w) as usize, pm.height().min(h) as usize);
    let src = pm.data();
    for y in 0..ph {
        let s0 = y * pm.width() as usize * 4;
        let d0 = y * w as usize * 4;
        out[d0..d0 + pw * 4].copy_from_slice(&src[s0..s0 + pw * 4]);
    }
    0
}

/// The app's files: HOME and the XDG folders under `path` (the app's private storage).
#[unsafe(no_mangle)]
pub extern "C" fn kd_home(path: *const c_char) {
    let home = PathBuf::from(arg(path));
    for (k, sub) in [("HOME", ""), ("XDG_CONFIG_HOME", "config"), ("XDG_DATA_HOME", "data"), ("XDG_CACHE_HOME", "cache")] {
        let d = if sub.is_empty() { home.clone() } else { home.join(sub) };
        let _ = std::fs::create_dir_all(&d);
        // (before the app starts any thread)
        unsafe { std::env::set_var(k, d) };
    }
}

/// The Content Hub's catalogue, searches and downloads (kn_ui contenthub::api). Holds no
/// document, so a worker thread may call it while the window works.
#[unsafe(no_mangle)]
pub extern "C" fn kd_hub(kind: *const c_char, a: *const c_char) -> *mut c_char {
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

/// Starts Keydeck with no presentation open (the theme chooser shows, as the desktop's start).
#[unsafe(no_mangle)]
pub extern "C" fn kd_init() {
    let _ = std::panic::catch_unwind(|| {
        let ctx = egui::Context::default();
        let mut app = App::with_ctx(&ctx, Vec::new());
        // pictures drawn when asked for (the front end draws on demand: none arrive later)
        app.renderer.set_async_images(false);
        // (table rows grow to fit what's typed into them, as the desktop's start)
        kn_model::table::set_all_rows_fit(true);
        HOST.with(|h| *h.borrow_mut() = Some(Host { ctx, app, copied: None, show: None, unsaved: None }));
    });
}

/// The last error's message.
#[unsafe(no_mangle)]
pub extern "C" fn kd_error() -> *mut c_char {
    out(ERR.with(|e| e.borrow().clone()))
}

#[unsafe(no_mangle)]
pub extern "C" fn kd_free_str(s: *mut c_char) {
    if !s.is_null() {
        drop(unsafe { CString::from_raw(s) });
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn kd_free_bytes(p: *mut u8, len: u32) {
    if !p.is_null() {
        drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(p, len as usize)) });
    }
}

// ---- files ----

/// Opens a presentation (as File > Open: Keynote or PowerPoint), in its own tab.
#[unsafe(no_mangle)]
pub extern "C" fn kd_open(path: *const c_char) -> i32 {
    let p = PathBuf::from(arg(path));
    with(|h| {
        let opened = if crate::pptx::is_pptx(&p) { crate::pptx::open_document(&p) } else { kn_model::Document::open(&p) };
        match opened {
            Ok(mut doc) => {
                if let Some(dir) = crate::themes::theme_dir() {
                    doc.find_layout_donor(&dir);
                }
                if crate::pptx::is_pptx(&p) {
                    doc.path = Some(p.clone());
                    match crate::pptx::mode(&p) {
                        Some((true, _)) => {
                            h.app.pptx_hybrid.insert(p.clone());
                        }
                        _ => {
                            h.app.pptx_hybrid.remove(&p);
                        }
                    }
                }
                h.app.make_room();
                h.app.set_document(doc);
                h.app.remember_path(&p);
                0
            }
            Err(kn_format::Error::NeedsPassword(_)) => fail("This presentation is protected with a password (not yet supported here)."),
            Err(e) => fail(e.to_string()),
        }
    })
}

/// A new presentation on a theme (`index` into kd_themes), as the theme chooser's Create.
#[unsafe(no_mangle)]
pub extern "C" fn kd_new(index: u32) -> i32 {
    with(|h| {
        let themes = crate::themes::available();
        let path = themes.get(index as usize).and_then(|t| t.1.clone());
        match crate::themes::open(&path).and_then(kn_model::Document::new_from_theme) {
            Ok(doc) => {
                h.app.new_document(doc);
                0
            }
            Err(e) => fail(e.to_string()),
        }
    })
}

/// The themes (the chooser's): [{name, preview (a PNG file made in the cache)}], and the
/// recent presentations.
#[unsafe(no_mangle)]
pub extern "C" fn kd_themes() -> *mut c_char {
    let themes: Vec<Value> = crate::themes::available()
        .iter()
        .map(|(name, path)| {
            let file = crate::themes::preview_file(name, path);
            json!({ "name": name, "preview": file.map(|f| f.to_string_lossy().into_owned()) })
        })
        .collect();
    let recent: Vec<Value> = App::recent_files().iter().filter(|p| p.is_file()).map(|p| json!({ "path": p.to_string_lossy(), "name": p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default() })).collect();
    json_out(Some(json!({ "themes": themes, "recent": recent, "default": "Daylight" })))
}

/// Saves to `path` (.key, or .pptx: PowerPoint) as File > Save would.
#[unsafe(no_mangle)]
pub extern "C" fn kd_save(path: *const c_char) -> i32 {
    let p = PathBuf::from(arg(path));
    with(|h| {
        if h.app.doc.is_none() {
            return fail("No presentation");
        }
        crate::textedit::commit(&mut h.app);
        h.app.save_to(p.clone());
        if p.exists() { 0 } else { fail(h.app.toast.as_ref().map(|t| t.0.clone()).unwrap_or_else(|| "Couldn't save".into())) }
    })
}

/// Exports to `path` by its extension (pdf, pptx, png of the current slide), as File > Export To.
#[unsafe(no_mangle)]
pub extern "C" fn kd_export(path: *const c_char) -> i32 {
    let p = PathBuf::from(arg(path));
    with(|h| {
        crate::textedit::commit(&mut h.app);
        let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        match ext.as_str() {
            "pptx" => {
                let Some(doc) = h.app.doc.as_ref() else { return fail("No presentation") };
                match crate::pptx::write(doc, &mut h.app.renderer, &p, None) {
                    Ok(()) => 0,
                    Err(e) => fail(e.to_string()),
                }
            }
            _ => {
                let Some(doc) = h.app.doc.as_ref() else { return fail("No presentation") };
                match crate::export::export_doc(doc, &mut h.app.renderer, &p) {
                    Ok(_) => 0,
                    Err(e) => fail(e),
                }
            }
        }
    })
}

// ---- the slides ----

/// The presentation's shape: {size: [w, h] (slide points), count, current, selected (the
/// navigator's), version (changes when anything drawn does)}.
#[unsafe(no_mangle)]
pub extern "C" fn kd_geometry() -> *mut c_char {
    json_out(with(|h| {
        let a = &mut h.app;
        let doc = a.doc.as_ref()?;
        let size = doc.slide_size();
        Some(json!({ "size": [size.w, size.h], "count": a.slides.len(), "current": a.cur, "selected": a.slide_sel, "key": slide_key(a, a.cur) }))
    }))
}

/// A key of what slide `index` shows in the editor (it changes when it does).
fn slide_key(a: &App, index: usize) -> u64 {
    let Some(s) = a.slides.get(index) else { return 0 };
    let rev = a.slide_rev(s.slide);
    let ed = a.canvas.editing.as_ref().filter(|_| index == a.cur).map(|e| e.version + 1).unwrap_or(0);
    // (52 bits: JSON numbers are doubles to the front end)
    let mut k = (s.slide.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ rev.wrapping_mul(0x1000_0001) ^ (ed << 40)) & ((1 << 52) - 1);
    if k == 0 {
        k = 1;
    }
    k
}

/// The slide navigator: [{key, skipped, depth, collapsed, hidden (inside a collapsed group),
/// number}] and the current and selected slides.
#[unsafe(no_mangle)]
pub extern "C" fn kd_navigator() -> *mut c_char {
    json_out(with(|h| {
        let a = &h.app;
        let mut hide_below: Option<u32> = None;
        let mut number = 0;
        let slides: Vec<Value> = a
            .slides
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let hidden = hide_below.is_some_and(|d| s.depth > d);
                if !hidden {
                    hide_below = if s.collapsed { Some(s.depth) } else { None };
                }
                if !s.skipped {
                    number += 1;
                }
                let rev = a.slide_rev(s.slide);
                json!({ "key": s.slide.wrapping_mul(0x9e37_79b9_7f4a_7c15).wrapping_add(rev.wrapping_mul(0x1000_0001)) & ((1 << 52) - 1), "skipped": s.skipped, "depth": s.depth, "collapsed": s.collapsed, "hidden": hidden, "number": if s.skipped { Value::Null } else { json!(number) }, "index": i })
            })
            .collect();
        Some(json!({ "slides": slides, "current": a.cur, "selected": a.slide_sel, "view": format!("{:?}", a.view) }))
    }))
}

/// Draws part of slide `index`: the slide points from (`x`, `y`) at `scale` pixels a point into
/// `buf` (`w` × `h` RGBA premultiplied). `mode` 0: everything; 1: without the objects being
/// dragged (canvasapi moving); 2: those objects alone, at their live places.
#[unsafe(no_mangle)]
pub extern "C" fn kd_render(index: u32, scale: f32, x: f32, y: f32, mode: u32, buf: *mut u8, w: u32, h: u32) -> i32 {
    with(|host| {
        let a = &mut host.app;
        let index = index as usize;
        let sc = if index == a.cur {
            if mode == 2 { crate::canvas::api::live_scene(a) } else { crate::canvas::api::scene(a) }
        } else {
            a.scene(index)
        };
        let Some(sc) = sc else { return -1 };
        let Some(doc) = a.doc.as_ref() else { return -1 };
        let Some(mut pm) = kn_render::tiny_skia::Pixmap::new(w.max(1), h.max(1)) else { return -1 };
        let t = kn_render::tiny_skia::Transform::from_row(scale, 0.0, 0.0, scale, -x * scale, -y * scale);
        let moving = if index == a.cur { crate::canvas::api::moving(a) } else { Vec::new() };
        let filter = match mode {
            1 => kn_render::Filter::Except(&moving),
            _ => kn_render::Filter::All,
        };
        let r = crate::textures::guarded("render", || {
            if mode == 2 {
                for d in sc.drawables.iter().filter(|d| moving.contains(&d.id)) {
                    a.renderer.draw_drawable(doc, d, &mut pm, t);
                }
            } else {
                a.renderer.draw_scene(doc, &sc, &mut pm, t, filter, true);
            }
        });
        if r.is_none() {
            return -1;
        }
        copy_out(&pm, buf, w, h)
    })
}

/// A layout's picture (`index` into the Add Slide popover's), `w` × `h` pixels.
#[unsafe(no_mangle)]
pub extern "C" fn kd_render_layout(index: u32, buf: *mut u8, w: u32, h: u32) -> i32 {
    with(|host| {
        let a = &mut host.app;
        let Some(doc) = a.doc.as_ref() else { return -1 };
        let Some((id, _)) = doc.layouts().get(index as usize).cloned() else { return -1 };
        let Some(sc) = doc.layout_scene(id) else { return -1 };
        let Some(pm) = a.renderer.render_scene(doc, &sc, w) else { return -1 };
        copy_out(&pm, buf, w, h)
    })
}

/// What's under a slide point (canvasapi hit_test); `tol` a finger's reach in slide points.
#[unsafe(no_mangle)]
pub extern "C" fn kd_hit(x: f32, y: f32, tol: f32) -> *mut c_char {
    json_out(with(|h| Some(crate::canvas::api::hit_test(&mut h.app, Point::new(x, y), tol))))
}

/// A tap on the slide (canvasapi tap).
#[unsafe(no_mangle)]
pub extern "C" fn kd_tap(x: f32, y: f32, tol: f32, count: u32, shift: bool) {
    with(|h| crate::canvas::api::tap(&mut h.app, Point::new(x, y), tol, count, shift))
}

/// A touch and hold on the slide (canvasapi hold).
#[unsafe(no_mangle)]
pub extern "C" fn kd_hold(x: f32, y: f32, tol: f32) {
    with(|h| crate::canvas::api::hold(&mut h.app, Point::new(x, y), tol))
}

/// A drag on the slide (canvasapi drag): kind 1 starts, 0 moves, 2 ends, 3 cancels.
#[unsafe(no_mangle)]
pub extern "C" fn kd_drag(kind: u32, x: f32, y: f32, tol: f32, shift: bool, ctrl: bool) {
    with(|h| crate::canvas::api::drag(&mut h.app, kind, Point::new(x, y), tol, shift, ctrl))
}

/// The caret or an end of the text selection to a slide point (canvasapi text_point).
#[unsafe(no_mangle)]
pub extern "C" fn kd_text_point(x: f32, y: f32, end: u32) {
    with(|h| crate::canvas::api::text_point(&mut h.app, Point::new(x, y), end))
}

/// What's drawn over the slide (canvasapi overlays).
#[unsafe(no_mangle)]
pub extern "C" fn kd_overlays() -> *mut c_char {
    json_out(with(|h| Some(crate::canvas::api::overlays(&mut h.app))))
}

/// The slide's right-click menu for what's selected; `slide` ≥ 0: a navigator slide's.
#[unsafe(no_mangle)]
pub extern "C" fn kd_context_menu(slide: i32) -> *mut c_char {
    json_out(with(|h| {
        if slide >= 0 {
            let i = slide as usize;
            if !h.app.slide_sel.contains(&i) {
                h.app.go_to(i);
            }
            h.app.nav_focus = true;
            h.app.sel.clear();
            let skipped = h.app.slides.get(i).is_some_and(|s| s.skipped);
            return Some(Value::Array(crate::menus::api::slide_items(&h.app, skipped)));
        }
        Some(Value::Array(crate::menus::api::context_items(&mut h.app)))
    }))
}

/// A toolbar popover's contents: View, Media (menu items), AddSlide (the layouts' names).
#[unsafe(no_mangle)]
pub extern "C" fn kd_popover(id: *const c_char) -> *mut c_char {
    let id = arg(id);
    json_out(with(|h| match id.as_str() {
        "View" => Some(Value::Array(crate::menus::api::view_items(&h.app))),
        "Media" => Some(Value::Array(crate::menus::api::media_items())),
        // the basic shapes' outlines (100 × 100), as the desktop's Shape popover draws them
        "Shape" => {
            use kn_model::PathEl;
            let shapes: Vec<Value> = kn_model::insert::BasicShape::ALL
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    let path = kn_model::path::from_path_source(&s.path_source(), 100.0, 100.0);
                    let els: Vec<Value> = path
                        .els
                        .iter()
                        .map(|e| match e {
                            PathEl::Move(p) => json!(["M", p.x, p.y]),
                            PathEl::Line(p) => json!(["L", p.x, p.y]),
                            PathEl::Quad(c, p) => json!(["Q", c.x, c.y, p.x, p.y]),
                            PathEl::Cubic(a, b, p) => json!(["C", a.x, a.y, b.x, b.y, p.x, p.y]),
                            PathEl::Close => json!(["Z"]),
                        })
                        .collect();
                    json!({ "action": format!("shape:basic:{i}"), "name": s.name(), "path": els })
                })
                .collect();
            Some(json!({ "shapes": shapes }))
        }
        "AddSlide" => {
            let doc = h.app.doc.as_ref()?;
            let s = doc.slide_size();
            Some(json!({ "aspect": s.h / s.w, "layouts": doc.layouts().iter().map(|(_, n)| n.clone()).collect::<Vec<_>>() }))
        }
        _ => None,
    }))
}

/// Runs an item's action: "cmd:<menu command>", "slide:<i>" (go to it; "slide:<i>:shift" adds
/// it to the selection), "move:<from>:<to>" (a navigator drag), "layout:<i>" (Add Slide),
/// "insert:<file>" (a picture, movie or sound), "play" (from the current slide).
#[unsafe(no_mangle)]
pub extern "C" fn kd_action(action: *const c_char) {
    let a = arg(action);
    with(|h| {
        let ctx = h.ctx.clone();
        let app = &mut h.app;
        if let Some(c) = a.strip_prefix("cmd:") {
            return run_command(app, &ctx, c);
        }
        if let Some(rest) = a.strip_prefix("slide:") {
            let (i, shift) = match rest.split_once(':') {
                Some((i, m)) => (i, m == "shift"),
                None => (rest, false),
            };
            let Ok(i) = i.parse::<usize>() else { return };
            if i >= app.slides.len() {
                return;
            }
            crate::textedit::commit(app);
            app.nav_focus = true;
            if shift {
                if let Some(k) = app.slide_sel.iter().position(|s| *s == i) {
                    if app.slide_sel.len() > 1 {
                        app.slide_sel.remove(k);
                        app.cur = app.slide_sel[0];
                    }
                } else {
                    app.slide_sel.push(i);
                    app.cur = i;
                }
                app.sel.clear();
            } else {
                app.go_to(i);
            }
            return;
        }
        if let Some(rest) = a.strip_prefix("move:") {
            let Some((f, t)) = rest.split_once(':') else { return };
            let (Ok(f), Ok(t)) = (f.parse::<usize>(), t.parse::<usize>()) else { return };
            if f == t || f >= app.slides.len() || t >= app.slides.len() || app.blocked_by_slide_groups() {
                return;
            }
            let Some(doc) = app.doc.as_mut() else { return };
            doc.begin(&mut app.hist, "Move Slide");
            let ok = doc.move_slide(&mut app.hist, f, t).is_ok();
            doc.end(&mut app.hist);
            if ok {
                app.bump_all();
                app.go_to(t);
            }
            return;
        }
        if let Some(i) = a.strip_prefix("layout:").and_then(|i| i.parse::<usize>().ok()) {
            let Some(layout) = app.doc.as_ref().and_then(|d| d.layouts().get(i).map(|l| l.0)) else { return };
            if app.blocked_by_slide_groups() {
                return;
            }
            let at = (app.cur + 1).min(app.slides.len());
            let Some(doc) = app.doc.as_mut() else { return };
            doc.begin(&mut app.hist, "Add Slide");
            let r = doc.new_slide_from_layout(&mut app.hist, layout, at);
            doc.end(&mut app.hist);
            match r {
                Ok(i) => {
                    app.bump_all();
                    app.go_to(i);
                }
                Err(e) => app.notify(format!("Couldn't add the slide: {e}")),
            }
            return;
        }
        // the Shape popover's (Text, Line, Arrow, a basic shape), as the Content Hub inserts them
        if let Some(k) = a.strip_prefix("shape:") {
            crate::editing::insert_from_hub(app, kn_ui::contenthub::Picked::App(k.to_string()));
            return;
        }
        if let Some(p) = a.strip_prefix("insert:") {
            app.insert_media_path(PathBuf::from(p));
            return;
        }
        // the tabs (two or more show a bar): "tab:select:i", "tab:new", "tab:close:i" (one with
        // unsaved changes asks first: kd_status "unsaved"), "tab:discard:i" (closed unsaved),
        // "tab:keep" (not closed after all); "close" the one shown
        let a = if a == "close" { format!("tab:close:{}", app.active_tab) } else { a };
        if let Some(t) = a.strip_prefix("tab:") {
            crate::textedit::commit(app);
            let (verb, i) = t.split_once(':').map(|(v, i)| (v, i.parse::<usize>().ok())).unwrap_or((t, None));
            match (verb, i) {
                ("select", Some(i)) if i < app.tabs.len() => app.select_tab(&ctx, i),
                ("new", _) => app.new_tab(),
                ("close", Some(i)) if i < app.tabs.len() => {
                    if app.tab_dirty(i) {
                        h.unsaved = Some(i);
                    } else {
                        app.close_tab(&ctx, i);
                    }
                }
                ("discard", Some(i)) if i < app.tabs.len() => {
                    h.unsaved = None;
                    app.close_tab(&ctx, i);
                }
                _ => h.unsaved = None,
            }
            return;
        }
        if a == "play" {
            h.show = Some((app.cur, 0));
            return;
        }
        if a == "msg:" {
            app.toast = None;
        }
    })
}

/// A menu command, as the desktop's menus run it.
fn run_command(app: &mut App, ctx: &egui::Context, c: &str) {
    match c {
        // slideshow commands play here (the front end draws the show)
        "Play Slideshow" | "Play" | "Rehearse Slideshow" => {
            app.start_play(app.cur, false);
        }
        _ => crate::menus::run(app, ctx, c),
    }
}

// ---- typing ----

fn keyboard(h: &mut Host, events: Vec<Event>) {
    let ctx = h.ctx.clone();
    let app = &mut h.app;
    let sc = crate::canvas::api::scene(app);
    let tl = sc.as_ref().and_then(|sc| {
        let id = app.canvas.editing.as_ref()?.id;
        let d = sc.drawables.iter().find(|d| d.id == id)?;
        crate::textedit::stored_layout(app.renderer.text_layout(d), d)
    });
    crate::textedit::handle_events(app, &ctx, tl.as_ref().map(|(t, _)| t), events);
}

/// Typed text (the on-screen keyboard): into the text being edited; with an object selected,
/// typing starts editing its text (Keynote).
#[unsafe(no_mangle)]
pub extern "C" fn kd_text(text: *const c_char) {
    let t = arg(text);
    with(|h| {
        if h.app.canvas.editing.is_none() {
            let &[id] = &h.app.sel[..] else { return };
            let Some(sc) = h.app.scene(h.app.cur) else { return };
            if !crate::textedit::begin(&mut h.app, &sc, id, None) {
                return;
            }
        }
        keyboard(h, vec![Event::Text(t)])
    })
}

/// A key: its name as egui's (Backspace, Delete, Enter, Tab, ArrowLeft, Escape…), with Shift,
/// Ctrl and Alt. Ctrl with a letter is a menu shortcut.
#[unsafe(no_mangle)]
pub extern "C" fn kd_key(name: *const c_char, shift: bool, ctrl: bool, alt: bool) {
    let name = arg(name);
    with(|h| {
        let Some(key) = egui::Key::from_name(&name) else { return };
        let modifiers = egui::Modifiers { shift, ctrl, alt, command: ctrl, ..Default::default() };
        let ctx = h.ctx.clone();
        if h.app.canvas.editing.is_none() || (ctrl && !matches!(key, egui::Key::ArrowLeft | egui::Key::ArrowRight | egui::Key::ArrowUp | egui::Key::ArrowDown | egui::Key::Home | egui::Key::End | egui::Key::Backspace | egui::Key::Delete | egui::Key::Z)) {
            // not typing: Delete removes the selection, Escape ends it, arrows nudge
            match key {
                egui::Key::Escape => {
                    h.app.sel.clear();
                }
                egui::Key::Backspace | egui::Key::Delete if !ctrl => crate::menus::run(&mut h.app, &ctx, "Delete"),
                egui::Key::ArrowLeft | egui::Key::ArrowRight | egui::Key::ArrowUp | egui::Key::ArrowDown if !h.app.sel.is_empty() && !ctrl => {
                    let step = if shift { 10.0 } else { 1.0 };
                    let (dx, dy) = match key {
                        egui::Key::ArrowLeft => (-step, 0.0),
                        egui::Key::ArrowRight => (step, 0.0),
                        egui::Key::ArrowUp => (0.0, -step),
                        _ => (0.0, step),
                    };
                    let ids = h.app.sel.clone();
                    let slide = h.app.current_slide().map(|s| s.slide);
                    if let (Some(doc), Some(slide)) = (h.app.doc.as_mut(), slide) {
                        doc.begin(&mut h.app.hist, "Move");
                        for id in &ids {
                            doc.move_by(&mut h.app.hist, *id, dx, dy);
                        }
                        doc.end(&mut h.app.hist);
                        h.app.bump(slide);
                    }
                }
                _ if ctrl => {
                    if let Some(cmd) = crate::menus::api::shortcut_command(modifiers, key) {
                        crate::menus::run(&mut h.app, &ctx, &cmd);
                    }
                }
                _ => {}
            }
            return;
        }
        if key == egui::Key::Escape {
            crate::textedit::commit(&mut h.app);
            return;
        }
        if ctrl && key == egui::Key::Z {
            crate::textedit::commit(&mut h.app);
            if shift { h.app.redo() } else { h.app.undo() }
            return;
        }
        keyboard(h, vec![Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }])
    })
}

/// Pasted text (the system clipboard), as Edit > Paste: into the text being edited, else a new
/// text box (or the objects Keydeck copied, when the clipboard holds what they copied).
#[unsafe(no_mangle)]
pub extern "C" fn kd_paste(text: *const c_char) {
    let t = arg(text);
    with(|h| {
        if h.app.canvas.editing.is_some() {
            return keyboard(h, vec![Event::Paste(t)]);
        }
        let ctx = h.ctx.clone();
        if h.app.clip.is_some() || h.app.slide_clip.is_some() {
            return crate::menus::run(&mut h.app, &ctx, "Paste");
        }
        crate::editing::paste_text_as_box(&mut h.app, &t);
    })
}

/// Text the last commands copied (Copy, Cut), once; null if none.
#[unsafe(no_mangle)]
pub extern "C" fn kd_copied() -> *mut c_char {
    match with(|h| h.copied.take()) {
        Some(s) => out(s),
        None => std::ptr::null_mut(),
    }
}

// ---- commands and state ----

/// The menu bar as data (menusapi as_data).
#[unsafe(no_mangle)]
pub extern "C" fn kd_menus() -> *mut c_char {
    json_out(with(|h| Some(crate::menus::api::as_data(&h.app))))
}

/// Runs a menu command by its name (as kd_menus gives it).
#[unsafe(no_mangle)]
pub extern "C" fn kd_run(command: *const c_char) {
    let c = arg(command);
    with(|h| {
        let ctx = h.ctx.clone();
        if let Some(p) = c.strip_prefix("open:") {
            drop(ctx);
            let p = PathBuf::from(p);
            let opened = if crate::pptx::is_pptx(&p) { crate::pptx::open_document(&p) } else { kn_model::Document::open(&p) };
            match opened {
                Ok(doc) => {
                    h.app.make_room();
                    h.app.set_document(doc);
                }
                Err(e) => h.app.notify(format!("Couldn't open {}: {e}", p.display())),
            }
            return;
        }
        run_command(&mut h.app, &ctx, &c);
    })
}

/// The toolbar's items as the front end names them (toolbar.rs ids).
fn tb_name(id: &str) -> String {
    use crate::toolbar::{FLEX, SPACE, VIEW, ZOOM};
    match id {
        VIEW => "View".into(),
        ZOOM => "Zoom".into(),
        SPACE => "Space".into(),
        FLEX => "FlexibleSpace".into(),
        _ => id.to_string(),
    }
}

/// A toolbar item without a popover, by its id (Play, Text, Comment, Format, Animate, Document…).
#[unsafe(no_mangle)]
pub extern "C" fn kd_toolbar(id: *const c_char) {
    let id = arg(id);
    with(|h| {
        let ctx = h.ctx.clone();
        let app = &mut h.app;
        use crate::app::InspectorKind;
        let pane = match id.as_str() {
            "Format" => Some(InspectorKind::Format),
            "Animate" => Some(InspectorKind::Animate),
            "Document" => Some(InspectorKind::Document),
            _ => None,
        };
        if let Some(k) = pane {
            // the pane's button: shows it, or hides the inspector when it's the one showing
            if app.show_inspector && app.inspector == k {
                app.show_inspector = false;
            } else {
                app.inspector = k;
                app.show_inspector = true;
            }
            return;
        }
        if id == "TSKToolbarItemPlay" {
            h.show = Some((app.cur, 0));
            return;
        }
        crate::toolbar::run(app, &ctx, &id);
    })
}

/// The presentation's state: title, edited, the message shown, undo / redo, the inspector,
/// typing, the selection.
#[unsafe(no_mangle)]
pub extern "C" fn kd_status() -> *mut c_char {
    json_out(with(|h| {
        let a = &h.app;
        Some(json!({
            "open": a.doc.is_some(),
            "title": a.doc.as_ref().map(|_| a.tab_name(a.active_tab)),
            "edited": a.doc.as_ref().is_some_and(|d| d.dirty),
            "message": a.toast.as_ref().map(|m| m.0.clone()),
            "canUndo": a.hist.can_undo(),
            "canRedo": a.hist.can_redo(),
            "inspector": a.show_inspector,
            "inspectorMode": format!("{:?}", a.inspector),
            "editingText": a.canvas.editing.is_some(),
            "objects": a.sel.len(),
            "navigator": a.view == crate::app::ViewMode::Navigator,
            "notes": a.show_notes,
            "playing": h.show.is_some(),
            "path": a.doc.as_ref().and_then(|d| d.path.as_ref()).map(|p| p.to_string_lossy().into_owned()),
            "pptx": a.doc.as_ref().and_then(|d| d.path.as_ref()).is_some_and(|p| crate::pptx::is_pptx(p)),
            "windows": crate::dlgapi::state(a),
            "tabs": (a.tabs.len() > 1).then(|| json!({ "tabs": (0..a.tabs.len()).map(|i| json!({ "name": a.tab_name(i), "edited": a.tab_dirty(i) })).collect::<Vec<_>>(), "active": a.active_tab })),
            "unsaved": h.unsaved.map(|i| json!({ "tab": i, "name": a.tab_name(i) })),
            "savedColors": a.settings.saved_colors,
        }))
    }))
}

/// The window's chrome as data: the look's colours (the desktop's palette), the title and the
/// toolbar's items and states.
#[unsafe(no_mangle)]
pub extern "C" fn kd_chrome() -> *mut c_char {
    out(with(|h| {
        let c = |c: egui::Color32| {
            let [r, g, b, al] = c.to_srgba_unmultiplied();
            (u32::from(al) << 24) | (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
        };
        use kn_ui::theme;
        let vis = h.ctx.style_of(if theme::is_light() { egui::Theme::Light } else { egui::Theme::Dark }).visuals.clone();
        let grays: serde_json::Map<String, Value> = (0..=255u8).map(|v| (format!("{v:02x}"), json!(c(theme::gray(v))))).collect();
        let sc = h.app.scene(h.app.cur);
        let mut items: Vec<Value> = Vec::new();
        for id in crate::toolbar::items(&h.app) {
            // the inspector switch: Format, Animate and Document side by side (one capsule)
            if id == crate::toolbar::INSPECTOR {
                use crate::app::InspectorKind;
                for (n, k, icon) in [("Format", InspectorKind::Format, "Format"), ("Animate", InspectorKind::Animate, "Animate"), ("Document", InspectorKind::Document, "Document")] {
                    items.push(json!({ "id": n, "label": n, "tip": "", "icon": icon, "enabled": true, "pressed": h.app.show_inspector && h.app.inspector == k, "popup": false }));
                }
                continue;
            }
            let d = crate::toolbar::def(&id);
            let popup = matches!(id.as_str(), crate::toolbar::VIEW | crate::toolbar::ZOOM | "KNMacToolbarItemAddSlide" | "TSKToolbarItemShape" | "TSKToolbarItemMedia" | "TSKToolbarItemChart" | "TSKToolbarItemTable");
            items.push(json!({
                "id": tb_name(&id),
                "label": crate::toolbar::label(&h.app, &id),
                "tip": d.map(|d| d.tip).unwrap_or(""),
                "icon": d.map(|d| format!("{:?}", d.icon)).unwrap_or_default(),
                "enabled": crate::toolbar::enabled(&mut h.app, &id, sc.as_ref()),
                "pressed": crate::toolbar::pressed(&h.app, &id),
                "popup": popup,
            }));
        }
        let a = &h.app;
        let dirty = a.doc.as_ref().is_some_and(|d| d.dirty);
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
                "header": c(theme::PANEL_BG()),
                "canvas": c(theme::CANVAS_BG()),
                "capsule": c(theme::gray(0x36)),
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
                "toolbarTint": c(kn_ui::glass::ToolbarBackground::tint()),
                "tabBar": c(if theme::is_light() { theme::tint(egui::Color32::from_rgb(0xe8, 0xe8, 0xe8)) } else { theme::tint(egui::Color32::from_rgb(0x35, 0x35, 0x35)) }),
                "tabActive": c(if theme::is_light() { theme::tint(egui::Color32::WHITE) } else { theme::tint(egui::Color32::from_rgb(0x4a, 0x4a, 0x4a)) }),
                "tabLine": c(if theme::is_light() { theme::tint(egui::Color32::from_gray(0xc8)) } else { theme::tint(egui::Color32::from_gray(0x48)) }),
            },
            "title": a.doc.as_ref().map(|_| a.tab_name(a.active_tab)).unwrap_or_else(|| "Keydeck".into()),
            "sub": if dirty { Some("Edited") } else { None },
            "zoom": 1.0,
            "showToolbar": a.show_toolbar,
            "glass": a.settings.liquid_glass,
            "translucent": a.settings.translucent_panes,
            "classic": a.settings.classic_look,
            "toolbarBackground": a.settings.toolbar_background,
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

/// The inspector as data: which pane shows and its controls' values (inspapi::state).
#[unsafe(no_mangle)]
pub extern "C" fn kd_inspector() -> *mut c_char {
    json_out(with(|h| Some(crate::inspapi::state(&mut h.app))))
}

/// Changes an inspector control: `key` as kd_inspector names it, `value` JSON.
#[unsafe(no_mangle)]
pub extern "C" fn kd_inspector_set(key: *const c_char, value: *const c_char) {
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

/// A picture of the document's (its data id) as a small PNG: its length into `len`; freed with
/// kd_free_bytes.
#[unsafe(no_mangle)]
pub extern "C" fn kd_thumbnail(data: u64, max: u32, len: *mut u32) -> *mut u8 {
    let png: Vec<u8> = with(|h| {
        let Some(bytes) = h.app.doc.as_ref().and_then(|d| d.data(data)) else { return Vec::new() };
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

// ---- the slideshow ----

/// The show as it stands: {slide, step, steps (its builds), count, transition: {effect,
/// duration, direction, automatic, delay} (into this slide), builds: [{effect, duration,
/// automatic, delay, withPrevious}] (the steps'), notes, end (past the last slide)}; null when
/// it isn't playing.
#[unsafe(no_mangle)]
pub extern "C" fn kd_show() -> *mut c_char {
    json_out(with(|h| {
        let (i, step) = h.show?;
        let count = h.app.slides.len();
        if i >= count {
            return Some(json!({ "end": true, "count": count }));
        }
        let sc = h.app.scene(i)?;
        let tr = sc.transition.as_ref().map(|t| json!({ "effect": t.effect, "duration": t.duration, "direction": t.direction, "automatic": t.automatic, "delay": t.delay }));
        let builds: Vec<Value> = sc
            .build_order
            .iter()
            .map(|s| {
                let b = sc.builds.iter().find(|b| b.id == s.build);
                json!({ "effect": b.map(|b| b.effect.clone()), "kind": b.map(|b| b.kind.clone()), "duration": s.duration, "automatic": s.automatic, "delay": s.delay, "withPrevious": s.with_previous })
            })
            .collect();
        let notes = h.app.doc.as_ref().and_then(|d| d.notes_storage(sc.slide).map(|s| d.storage_text(s))).unwrap_or_default();
        Some(json!({ "slide": i, "step": step, "steps": builds.len(), "count": count, "transition": tr, "builds": builds, "notes": notes, "end": false, "size": [sc.size.w, sc.size.h] }))
    }))
}

/// Moves the show: `how` "start:<i>", "next", "prev", "go:<i>", "end".
#[unsafe(no_mangle)]
pub extern "C" fn kd_show_go(how: *const c_char) {
    let how = arg(how);
    with(|h| {
        let a = &mut h.app;
        let n = a.slides.len();
        let visible = |a: &App, i: usize| a.slides.get(i).is_some_and(|s| !s.skipped);
        if let Some(i) = how.strip_prefix("start:").and_then(|i| i.parse::<usize>().ok()) {
            crate::textedit::commit(a);
            let mut i = i.min(n.saturating_sub(1));
            while i < n && !visible(a, i) {
                i += 1;
            }
            h.show = Some((i, 0));
            return;
        }
        if how == "end" {
            h.show = None;
            return;
        }
        let Some((i, step)) = h.show else { return };
        let steps = a.scene(i).map(|s| s.build_order.len()).unwrap_or(0);
        h.show = Some(match how.as_str() {
            "next" if step < steps => (i, step + 1),
            "next" => {
                let mut j = i + 1;
                while j < n && !visible(a, j) {
                    j += 1;
                }
                (j.min(n), 0)
            }
            "prev" if i >= n => (n.saturating_sub(1), a.scene(n.saturating_sub(1)).map(|s| s.build_order.len()).unwrap_or(0)),
            "prev" if step > 0 => (i, step - 1),
            "prev" => {
                let mut j = i;
                while j > 0 {
                    j -= 1;
                    if visible(a, j) {
                        let s = a.scene(j).map(|s| s.build_order.len()).unwrap_or(0);
                        return h.show = Some((j, s));
                    }
                }
                (i, 0)
            }
            g => match g.strip_prefix("go:").and_then(|i| i.parse::<usize>().ok()) {
                Some(j) if j < n => (j, 0),
                _ => (i, step),
            },
        });
    })
}

/// Slide `index` as the show draws it at build `step` (what's built in so far), `w` × `h` pixels.
#[unsafe(no_mangle)]
pub extern "C" fn kd_show_frame(index: u32, step: u32, buf: *mut u8, w: u32, h: u32) -> i32 {
    with(|host| {
        let a = &mut host.app;
        let Some(sc) = a.scene(index as usize) else { return -1 };
        let st = crate::builds::state_at(&sc, step as usize);
        let sc2 = st.scene_for(&sc);
        let sc = sc2.as_ref().unwrap_or(&sc);
        let Some(doc) = a.doc.as_ref() else { return -1 };
        let Some(mut pm) = kn_render::tiny_skia::Pixmap::new(w.max(1), h.max(1)) else { return -1 };
        let scale = w as f32 / sc.size.w;
        let r = crate::textures::guarded("slideshow", || {
            crate::builds::draw_state(&mut a.renderer, doc, sc, &st, &mut pm, kn_render::tiny_skia::Transform::from_scale(scale, scale), &|_| true, true);
        });
        if r.is_none() {
            return -1;
        }
        copy_out(&pm, buf, w, h)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(p: *mut c_char) -> Value {
        let v = serde_json::from_str(&unsafe { CStr::from_ptr(p) }.to_string_lossy()).unwrap_or(Value::Null);
        kd_free_str(p);
        v
    }

    fn c(s: &str) -> CString {
        CString::new(s).unwrap()
    }

    /// A new presentation from a theme, a text box typed into, a drag, saved as Keynote and as
    /// PowerPoint and opened again, the show stepped through: all without any egui pass.
    #[test]
    fn presentation_api() {
        let home = std::env::temp_dir().join("keydeck-ffi-test");
        let _ = std::fs::remove_dir_all(&home);
        kd_home(c(&home.to_string_lossy()).as_ptr());
        kd_init();
        assert_eq!(kd_new(0), 0);
        let g = json(kd_geometry());
        assert!(g["count"].as_u64().unwrap() >= 1, "{g}");
        // Insert > Text Box, then typing into it
        kd_run(c("Text").as_ptr());
        let o = json(kd_overlays());
        assert_eq!(o["selection"].as_array().map(|a| a.len()), Some(1), "a text box selected: {o}");
        kd_text(c("Hello slides").as_ptr());
        let o = json(kd_overlays());
        assert!(o["editing"]["caret"].is_array(), "a caret: {o}");
        kd_key(c("Escape").as_ptr(), false, false, false);
        // its frame's middle, dragged 100 points right
        let o = json(kd_overlays());
        let cs: Vec<(f32, f32)> = o["selection"][0]["corners"].as_array().unwrap().iter().map(|p| (p[0].as_f64().unwrap() as f32, p[1].as_f64().unwrap() as f32)).collect();
        let (mx, my) = ((cs[0].0 + cs[2].0) / 2.0, (cs[0].1 + cs[2].1) / 2.0);
        kd_drag(1, mx, my, 10.0, false, false);
        kd_drag(0, mx + 100.0, my, 10.0, false, false);
        kd_drag(2, mx + 100.0, my, 10.0, false, false);
        let o2 = json(kd_overlays());
        let x2 = o2["selection"][0]["corners"][0][0].as_f64().unwrap() as f32;
        assert!((x2 - cs[0].0 - 100.0).abs() < 0.5, "moved: {o2}");
        // a picture of the slide
        let size = &g["size"];
        let (w, h) = (192u32, (192.0 * size[1].as_f64().unwrap() / size[0].as_f64().unwrap()) as u32);
        let mut buf = vec![0u8; (w * h * 4) as usize];
        assert_eq!(kd_render(0, w as f32 / size[0].as_f64().unwrap() as f32, 0.0, 0.0, 0, buf.as_mut_ptr(), w, h), 0);
        // saved both ways and opened again
        for ext in ["key", "pptx"] {
            let p = home.join(format!("t.{ext}"));
            assert_eq!(kd_save(c(&p.to_string_lossy()).as_ptr()), 0, "save {ext}: {}", unsafe { CStr::from_ptr(kd_error()) }.to_string_lossy());
            assert_eq!(kd_open(c(&p.to_string_lossy()).as_ptr()), 0, "open {ext}");
            let st = json(kd_status());
            assert_eq!(st["open"], true);
        }
        // the show: start, next until the end
        kd_show_go(c("start:0").as_ptr());
        let s = json(kd_show());
        assert_eq!(s["slide"], 0, "{s}");
        let mut n = 0;
        while json(kd_show())["end"] != true && n < 50 {
            kd_show_go(c("next").as_ptr());
            n += 1;
        }
        assert!(json(kd_show())["end"] == true);
        kd_show_go(c("end").as_ptr());
        let ch = json(kd_chrome());
        assert!(ch["toolbar"].as_array().unwrap().len() > 5);
        let insp = json(kd_inspector());
        assert!(insp["pane"].is_string(), "{insp}");
    }
}
