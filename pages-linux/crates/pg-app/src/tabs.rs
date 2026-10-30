//! Document tabs (Pages' window tabs, as Keydeck's): every open document is a tab of the one
//! window. The active document lives in the App as before; the others wait as a [`DocTab`].
//! File > Open, Open Recent, files dropped on the window and extra files on the command line
//! open in a new tab (or go to the tab already showing that file).

use std::path::{Path, PathBuf};

use egui::{Color32, Rect};
use kn_ui::theme;
use pg_core::kn_model::edit::History;
use pg_core::Pages;

use crate::app::{App, Cursor, Sel, Sidebar};

/// Everything that belongs to one document.
pub struct DocTab {
    wb: Option<Pages>,
    hist: History,
    cursor: Option<Cursor>,
    typing_props: Vec<crate::text::CharChange>,
    sel: Option<Sel>,
    min_pages: usize,
    untitled_name: Option<String>,
    find: Option<(String, Vec<(usize, usize)>, usize)>,
    chart_edit: Option<crate::chartedit::ChartEdit>,
    points: crate::points::PointEdit,
    mask_edit: Option<pg_core::kn_format::ObjId>,
    desc_buf: Option<(pg_core::kn_format::ObjId, String)>,
    comment_edit: Option<(Option<pg_core::kn_format::ObjId>, String)>,
    equation_target: Option<pg_core::kn_format::ObjId>,
    ruler: bool,
    word_count: bool,
    sidebar: Sidebar,
    show_comments: bool,
    flow_cache: pg_core::flow::Cache,
    zoom: f32,
    scroll: egui::Vec2,
    two_up: bool,
    fit: crate::canvas::Fit,
    obj: Option<pg_core::kn_format::ObjId>,
    objs: Vec<pg_core::kn_format::ObjId>,
    word: Option<crate::app::WordMode>,
}

/// The window's tabs: one slot per tab, the active one's empty (its document is in the App).
pub struct Tabs {
    pub slots: Vec<Option<DocTab>>,
    pub active: usize,
    pub show_bar: bool,
    pub show_all: bool,
    /// Tabs to close, one by one (a changed one asks first); quitting closes them all.
    closing: Vec<usize>,
    quitting: bool,
}

impl Default for Tabs {
    fn default() -> Self {
        Tabs { slots: vec![None], active: 0, show_bar: true, show_all: false, closing: Vec::new(), quitting: false }
    }
}

impl Tabs {
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// A close question or Show All Tabs is up (the app's other shortcuts wait).
    pub fn modal(&self) -> bool {
        !self.closing.is_empty() || self.show_all
    }
}

/// Move the active document's state out of the App.
fn stash(app: &mut App) -> DocTab {
    crate::text::finish_typing(app);
    crate::tables::commit(app);
    let t = DocTab {
        wb: app.wb.take(),
        hist: std::mem::take(&mut app.hist),
        cursor: app.cursor.take(),
        typing_props: std::mem::take(&mut app.typing_props),
        sel: app.sel.take(),
        min_pages: app.min_pages,
        untitled_name: app.untitled_name.take(),
        find: app.find.take(),
        chart_edit: app.chart_edit.take(),
        points: std::mem::take(&mut app.points),
        mask_edit: app.mask_edit.take(),
        desc_buf: app.desc_buf.take(),
        comment_edit: app.comment_edit.take(),
        equation_target: app.equation_target.take(),
        ruler: app.ruler,
        word_count: app.word_count,
        sidebar: app.sidebar,
        show_comments: app.show_comments,
        flow_cache: std::mem::take(&mut app.flow_cache),
        zoom: app.canvas.zoom,
        scroll: app.canvas.scroll,
        two_up: app.canvas.two_up,
        fit: app.canvas.fit,
        obj: app.canvas.obj,
        objs: std::mem::take(&mut app.canvas.objs),
        word: app.word.take(),
    };
    forget_layout(app);
    t
}

fn restore(app: &mut App, t: DocTab) {
    app.wb = t.wb;
    app.hist = t.hist;
    app.cursor = t.cursor;
    app.typing_props = t.typing_props;
    app.sel = t.sel;
    app.min_pages = t.min_pages;
    app.untitled_name = t.untitled_name;
    app.find = t.find;
    app.chart_edit = t.chart_edit;
    app.points = t.points;
    app.mask_edit = t.mask_edit;
    app.desc_buf = t.desc_buf;
    app.comment_edit = t.comment_edit;
    app.equation_target = t.equation_target;
    app.ruler = t.ruler;
    app.word_count = t.word_count;
    app.sidebar = t.sidebar;
    app.show_comments = t.show_comments;
    app.flow_cache = t.flow_cache;
    forget_layout(app);
    app.canvas.zoom = t.zoom;
    app.canvas.scroll = t.scroll;
    app.canvas.two_up = t.two_up;
    app.canvas.fit = t.fit;
    app.canvas.obj = t.obj;
    app.canvas.objs = t.objs;
    app.word = t.word;
}

/// What was laid out and drawn for the document that was active (object ids differ between
/// documents, so every cache keyed by them goes).
fn forget_layout(app: &mut App) {
    app.layout = None;
    app.drawables.clear();
    app.layout_drawables.clear();
    app.obj_page.clear();
    let (zoom, scroll) = (app.canvas.zoom, app.canvas.scroll);
    app.canvas.reset();
    app.canvas.tiles.renderer.clear_caches();
    app.canvas.zoom = zoom;
    app.canvas.scroll = scroll;
    app.version += 1;
}

/// A new tab with no document becomes active.
fn new_empty_tab(app: &mut App) {
    let t = stash(app);
    let cur = app.tabs.active;
    app.tabs.slots[cur] = Some(t);
    app.tabs.slots.push(None);
    app.tabs.active = app.tabs.slots.len() - 1;
    app.canvas.scroll = egui::Vec2::ZERO;
    app.canvas.zoom = app.settings.default_zoom;
}

/// File > New with a document open: a new tab with the template chooser.
pub fn new_tab(app: &mut App) {
    new_empty_tab(app);
    app.new_document(false);
    app.chooser = app.settings.show_chooser && !app.automated;
}

/// Go to tab `i`.
pub fn select(app: &mut App, i: usize) {
    if i == app.tabs.active || i >= app.tabs.len() {
        return;
    }
    let t = stash(app);
    let cur = app.tabs.active;
    app.tabs.slots[cur] = Some(t);
    if let Some(next) = app.tabs.slots[i].take() {
        restore(app, next);
    }
    app.tabs.active = i;
}

fn same_file(a: &Path, b: &Path) -> bool {
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    a == b || canon(a) == canon(b)
}

fn path_of(app: &App, i: usize) -> Option<PathBuf> {
    if i == app.tabs.active {
        app.word.as_ref().map(|w| w.path.clone()).or_else(|| app.doc().and_then(|d| d.path.clone()))
    } else {
        let t = app.tabs.slots.get(i)?.as_ref()?;
        t.word.as_ref().map(|w| w.path.clone()).or_else(|| t.wb.as_ref()?.doc.path.clone())
    }
}

/// The active tab holds only an untouched new document (a file opened replaces it).
fn active_is_blank(app: &App) -> bool {
    match app.doc() {
        None => true,
        Some(d) => d.path.is_none() && !d.dirty && app.untitled_name.is_none() && app.word.is_none(),
    }
}

/// Open `p` in its own tab: the tab already showing it, the active tab when that one is an
/// untouched new document, else a new tab.
pub fn open_in_tab(app: &mut App, p: &Path) {
    if let Some(i) = (0..app.tabs.len()).find(|&i| path_of(app, i).is_some_and(|q| same_file(&q, p))) {
        select(app, i);
        return;
    }
    if active_is_blank(app) {
        app.open(p);
        return;
    }
    let back = app.tabs.active;
    new_empty_tab(app);
    app.open(p);
    // it didn't open (and isn't waiting for its password): back to where we were
    if app.wb.is_none() && !app.password_pending() {
        let i = app.tabs.active;
        app.tabs.slots.remove(i);
        app.tabs.active = back.min(app.tabs.len() - 1);
        if let Some(t) = app.tabs.slots[app.tabs.active].take() {
            restore(app, t);
        }
    }
}

pub fn dirty(app: &App, i: usize) -> bool {
    if i == app.tabs.active {
        app.doc().is_some_and(|d| d.dirty)
    } else {
        app.tabs.slots.get(i).and_then(|t| t.as_ref()).and_then(|t| t.wb.as_ref()).is_some_and(|w| w.doc.dirty)
    }
}

fn name(app: &App, i: usize) -> String {
    if i == app.tabs.active {
        return app.title();
    }
    let Some(t) = app.tabs.slots.get(i).and_then(|t| t.as_ref()) else { return "Untitled".into() };
    match t.wb.as_ref().and_then(|w| w.doc.path.as_ref()) {
        Some(p) => p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        None => t.untitled_name.clone().unwrap_or_else(|| "Untitled".into()),
    }
}

/// Close tab `i` now (no questions); the last tab closes the window.
fn close_now(app: &mut App, ctx: &egui::Context, i: usize) {
    if i >= app.tabs.len() {
        return;
    }
    if app.tabs.len() == 1 {
        if let Some(w) = &mut app.wb {
            w.doc.dirty = false;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        return;
    }
    if i == app.tabs.active {
        let _ = stash(app);
        app.tabs.slots.remove(i);
        let n = i.min(app.tabs.len() - 1);
        if let Some(t) = app.tabs.slots[n].take() {
            restore(app, t);
        }
        app.tabs.active = n;
    } else {
        app.tabs.slots.remove(i);
        if i < app.tabs.active {
            app.tabs.active -= 1;
        }
    }
    // later closes in the queue point past this one
    for c in app.tabs.closing.iter_mut() {
        if *c > i {
            *c -= 1;
        }
    }
}

/// File > Close (the active tab), Close All / Quit (every tab): changed ones ask first.
pub fn request_close(app: &mut App, all: bool) {
    if all {
        app.tabs.closing = (0..app.tabs.len()).collect();
        app.tabs.quitting = true;
    } else {
        app.tabs.closing = vec![app.tabs.active];
    }
}

/// The window's close button: close every tab, asking about changed ones. True when the
/// close should be held back for that.
pub fn on_close_requested(app: &mut App) -> bool {
    if app.automated || !app.tabs.closing.is_empty() {
        return !app.tabs.closing.is_empty();
    }
    if (0..app.tabs.len()).any(|i| dirty(app, i)) {
        request_close(app, true);
        return true;
    }
    false
}

/// Work through the tabs to close: clean ones close at once, a changed one shows the
/// "save the changes" question (its tab brought to the front).
pub fn closing_ui(app: &mut App, ctx: &egui::Context) {
    while let Some(&i) = app.tabs.closing.last() {
        if i >= app.tabs.len() {
            app.tabs.closing.pop();
            continue;
        }
        if !dirty(app, i) || (app.automated && app.tabs.quitting) {
            app.tabs.closing.pop();
            close_now(app, ctx, i);
            continue;
        }
        select(app, i);
        let mut choice = None;
        egui::Window::new("Unsaved changes").collapsible(false).resizable(false).pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).show(ctx, |ui| {
            ui.label(format!("Do you want to save the changes to “{}”?", app.title()));
            ui.horizontal(|ui| {
                if ui.button("Don't Save").clicked() {
                    choice = Some(0);
                }
                if ui.button("Cancel").clicked() {
                    choice = Some(1);
                }
                if ui.button("Save").clicked() {
                    choice = Some(2);
                }
            });
        });
        match choice {
            Some(0) => {
                if let Some(w) = &mut app.wb {
                    w.doc.dirty = false;
                }
            }
            Some(1) => {
                app.tabs.closing.clear();
                app.tabs.quitting = false;
            }
            Some(2) => {
                app.save(false);
                // saving can be cancelled (Save As for a new document): stop there
                if app.doc().is_some_and(|d| d.dirty) {
                    app.tabs.closing.clear();
                    app.tabs.quitting = false;
                }
            }
            _ => {}
        }
        return;
    }
    if app.tabs.quitting {
        app.tabs.quitting = false;
        if let Some(w) = &mut app.wb {
            w.doc.dirty = false;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}

/// Window menu tab commands.
pub fn command(app: &mut App, name: &str) -> bool {
    let n = app.tabs.len();
    match name {
        "Show Tab Bar" => app.tabs.show_bar = !app.tabs.show_bar,
        "Show All Tabs" => app.tabs.show_all = true,
        "Show Next Tab" => select(app, (app.tabs.active + 1) % n),
        "Show Previous Tab" => select(app, (app.tabs.active + n - 1) % n),
        "Move Tab to New Window" => {
            // saved documents reopen in a window of their own; this tab closes
            let Some(p) = app.doc().and_then(|d| d.path.clone()).filter(|_| n > 1 && !dirty(app, app.tabs.active)) else {
                app.notify("Save the document first, then move its tab to a new window.");
                return true;
            };
            crate::fileops::open_in_new_window(&p);
            app.tabs.closing = vec![app.tabs.active];
        }
        _ => return false,
    }
    true
}

pub fn enabled(app: &App, name: &str) -> Option<bool> {
    Some(match name {
        "Show Next Tab" | "Show Previous Tab" | "Move Tab to New Window" => app.tabs.len() > 1,
        "Show Tab Bar" | "Show All Tabs" => true,
        _ => return None,
    })
}

/// The tab bar's height (shown with two tabs or more, unless hidden).
pub fn bar_height(app: &App) -> f32 {
    if app.tabs.len() > 1 && app.tabs.show_bar { 30.0 } else { 0.0 }
}

/// macOS-style tab bar: equal tabs in a rounded bar, the active one a lighter capsule, a close
/// button on hover (and middle-click closes), "+" for a new document.
pub fn bar(app: &mut App, ui: &mut egui::Ui) {
    let full = ui.max_rect().shrink2(egui::vec2(8.0, 3.0));
    let light = theme::is_light();
    let plus = Rect::from_center_size(egui::pos2(full.max.x - full.height() / 2.0, full.center().y), egui::vec2(full.height(), full.height()));
    let bar = Rect::from_min_max(full.min, egui::pos2(plus.min.x - 6.0, full.max.y));
    let n = app.tabs.len();
    let w = bar.width() / n as f32;
    let p = ui.painter().clone();
    let r = bar.height() / 2.0;
    let base = if light { theme::tint(Color32::from_rgb(0xe8, 0xe8, 0xe8)) } else { theme::tint(Color32::from_rgb(0x35, 0x35, 0x35)) };
    p.rect_filled(bar, r, base);
    let (mut pick, mut close) = (None, None);
    for i in 0..n {
        let t = Rect::from_min_size(bar.min + egui::vec2(i as f32 * w, 0.0), egui::vec2(w, bar.height()));
        let resp = ui.interact(t, egui::Id::unique(("doc-tab", i)), egui::Sense::click());
        let active = i == app.tabs.active;
        if active {
            p.rect_filled(t.shrink(2.0), r - 2.0, if light { theme::tint(Color32::WHITE) } else { theme::tint(Color32::from_rgb(0x4a, 0x4a, 0x4a)) });
        } else if resp.hovered() {
            p.rect_filled(t.shrink(2.0), r - 2.0, if light { Color32::from_black_alpha(10) } else { Color32::from_white_alpha(8) });
        }
        if i > 0 && !active && i != app.tabs.active + 1 {
            p.vline(t.min.x, t.y_range().shrink(6.0), egui::Stroke::new(1.0, if light { theme::tint(Color32::from_gray(0xc8)) } else { theme::tint(Color32::from_gray(0x48)) }));
        }
        let label = format!("{}{}", name(app, i), if dirty(app, i) { " \u{2014} Edited" } else { "" });
        let galley = p.layout(label, egui::FontId::proportional(12.0), if active { theme::TEXT() } else { theme::TEXT_DIM() }, (w - 44.0).max(20.0));
        let at = t.center() - galley.size() / 2.0;
        p.with_clip_rect(t.shrink2(egui::vec2(24.0, 0.0))).galley(at, galley, Color32::WHITE);
        let cr = Rect::from_center_size(t.left_center() + egui::vec2(14.0, 0.0), egui::vec2(16.0, 16.0));
        let ch = ui.interact(cr, egui::Id::unique(("doc-tab-x", i)), egui::Sense::click());
        if resp.hovered() || ch.hovered() {
            if ch.hovered() {
                p.circle_filled(cr.center(), 8.0, theme::gray(0x44));
            }
            let k = 3.5;
            let st = egui::Stroke::new(1.2, theme::TEXT_DIM());
            p.line_segment([cr.center() - egui::vec2(k, k), cr.center() + egui::vec2(k, k)], st);
            p.line_segment([cr.center() + egui::vec2(-k, k), cr.center() + egui::vec2(k, -k)], st);
        }
        if ch.clicked() || resp.middle_clicked() {
            close = Some(i);
        } else if resp.clicked() {
            pick = Some(i);
        }
    }
    let pl = ui.interact(plus, egui::Id::unique("doc-tab-plus"), egui::Sense::click()).on_hover_text("New Document");
    p.circle_filled(plus.center(), plus.width() / 2.0, if pl.hovered() { theme::gray(0x48) } else { base });
    let st = egui::Stroke::new(1.4, theme::TEXT());
    p.line_segment([plus.center() - egui::vec2(5.0, 0.0), plus.center() + egui::vec2(5.0, 0.0)], st);
    p.line_segment([plus.center() - egui::vec2(0.0, 5.0), plus.center() + egui::vec2(0.0, 5.0)], st);
    if pl.clicked() {
        new_tab(app);
    }
    if let Some(i) = pick {
        select(app, i);
    }
    if let Some(i) = close {
        app.tabs.closing = vec![i];
    }
}

/// Window > Show All Tabs: every open document as a card; click one to go to it.
pub fn all_tabs_ui(app: &mut App, ctx: &egui::Context) {
    if !app.tabs.show_all {
        return;
    }
    let mut pick = None;
    let mut done = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    egui::Modal::new(egui::Id::unique("all-tabs")).show(ctx, |ui| {
        ui.set_width(560.0);
        ui.label(theme::bold("All Tabs").size(16.0));
        ui.add_space(8.0);
        egui::Grid::new("all-tabs-grid").spacing([12.0, 12.0]).show(ui, |ui| {
            for i in 0..app.tabs.len() {
                let b = egui::Button::new(theme::bold(name(app, i))).min_size(egui::vec2(170.0, 100.0)).selected(i == app.tabs.active);
                if ui.add(b).clicked() {
                    pick = Some(i);
                }
                if i % 3 == 2 {
                    ui.end_row();
                }
            }
        });
        ui.add_space(8.0);
        if ui.button("Done").clicked() {
            done = true;
        }
    });
    if let Some(i) = pick {
        select(app, i);
        done = true;
    }
    if done {
        app.tabs.show_all = false;
    }
}
