//! Alignment guides you add (View > Guides): drag down from the ruler for a horizontal guide,
//! or right-click a page > Add Horizontal / Vertical Guide; drag a guide to move it, off the
//! page to remove it. Objects snap to them. Stored with the page (pg-core guides).

use egui::{pos2, vec2, Color32, Pos2, Rect, Stroke};
use pg_core::guides::Guide;

use crate::app::App;
use crate::canvas::page_rect;

/// Keynote / Pages guide yellow.
const COLOR: Color32 = Color32::from_rgb(0xee, 0xca, 0x00);

struct Drag {
    page: usize,
    /// The guide being moved (None: a new one).
    index: Option<usize>,
    vertical: bool,
    pos: f32,
}

struct State {
    shown: bool,
    template_shown: bool,
    drag: Option<Drag>,
    /// Where the page was right-clicked (page, page point), for the context menu.
    menu_at: Option<(usize, Pos2)>,
    /// Page guides read for this document version.
    cache: (u64, std::collections::HashMap<usize, (Vec<Guide>, Vec<Guide>)>),
}

thread_local! {
    static STATE: std::cell::RefCell<State> = std::cell::RefCell::new(State { shown: true, template_shown: true, drag: None, menu_at: None, cache: (u64::MAX, Default::default()) });
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| f(&mut s.borrow_mut()))
}

pub fn shown() -> bool {
    with(|s| s.shown)
}

pub fn template_shown() -> bool {
    with(|s| s.template_shown)
}

/// Page `pi`'s own guides and its template's (each empty when hidden).
pub fn on_page(app: &App, pi: usize) -> (Vec<Guide>, Vec<Guide>) {
    let (shown, tshown) = with(|s| (s.shown, s.template_shown));
    if !shown && !tshown {
        return Default::default();
    }
    let (Some(w), Some(l)) = (app.wb.as_ref(), app.layout.as_ref()) else { return Default::default() };
    let (own, tpl) = with(|s| {
        if s.cache.0 != app.version {
            s.cache = (app.version, Default::default());
        }
        s.cache.1.entry(pi).or_insert_with(|| (w.page_guides(l, pi), w.template_guides(l, pi))).clone()
    });
    (if shown { own } else { Vec::new() }, if tshown { tpl } else { Vec::new() })
}

/// The snapping targets for objects moved on page `pi`: (0, x) vertical, (1, y) horizontal.
pub fn snap_lines(app: &App, pi: usize) -> Vec<(usize, f32)> {
    let (own, tpl) = on_page(app, pi);
    own.iter().chain(&tpl).map(|g| (usize::from(!g.vertical), g.pos)).collect()
}

/// A drag that began in the ruler away from its markers: a new horizontal guide.
pub fn start_from_ruler(app: &App) {
    with(|s| {
        s.shown = true;
        s.drag = Some(Drag { page: app.current_page(), index: None, vertical: false, pos: 0.0 });
    });
}

/// Draw the guides of the pages in view and handle grabbing / moving them. Returns true when
/// the pointer is the guides' (the canvas leaves it alone).
pub fn ui(app: &mut App, ui: &egui::Ui, resp: &egui::Response) -> bool {
    let Some(l) = app.layout.as_ref() else { return false };
    let view = app.canvas.view_rect();
    let zoom = app.canvas.zoom;
    let pages: Vec<usize> = (0..l.page_count()).filter(|i| {
        let r = page_rect(&app.canvas, l, *i);
        Rect::from_two_pos(app.canvas.to_screen(r.min), app.canvas.to_screen(r.max)).intersects(view)
    }).collect();
    let painter = ui.painter_at(view);
    let screen_rect = |app: &App, pi: usize| {
        let l = app.layout.as_ref().unwrap();
        let r = page_rect(&app.canvas, l, pi);
        Rect::from_two_pos(app.canvas.to_screen(r.min), app.canvas.to_screen(r.max))
    };
    let line = |r: Rect, g: &Guide| if g.vertical { [pos2(r.min.x + g.pos * zoom, r.min.y), pos2(r.min.x + g.pos * zoom, r.max.y)] } else { [pos2(r.min.x, r.min.y + g.pos * zoom), pos2(r.max.x, r.min.y + g.pos * zoom)] };
    let moving = with(|s| s.drag.as_ref().and_then(|d| d.index.map(|i| (d.page, i))));
    let mut lines: Vec<(usize, usize, Guide)> = Vec::new();
    for &pi in &pages {
        let r = screen_rect(app, pi);
        let (own, tpl) = on_page(app, pi);
        for g in &tpl {
            painter.line_segment(line(r, g), Stroke::new(1.0, COLOR.gamma_multiply(0.55)));
        }
        for (i, g) in own.iter().enumerate() {
            if moving != Some((pi, i)) {
                painter.line_segment(line(r, g), Stroke::new(1.0, COLOR));
            }
            lines.push((pi, i, *g));
        }
    }
    let pointer = ui.input(|i| i.pointer.latest_pos());
    if resp.secondary_clicked() {
        let at = pointer.and_then(|p| app.canvas.hit_page(app.layout.as_ref().unwrap(), p));
        with(|s| s.menu_at = at);
    }

    // a guide being dragged follows the pointer until the button comes up
    if with(|s| s.drag.is_some()) {
        let down = ui.input(|i| i.pointer.primary_down());
        let Some(p) = pointer else { return true };
        let l = app.layout.as_ref().unwrap();
        let (page, q) = app.canvas.near_page(l, p);
        let ps = vec2(l.setup.width, l.setup.height);
        let (vertical, index, from) = with(|s| {
            let d = s.drag.as_mut().unwrap();
            if d.index.is_none() {
                d.page = page;
            }
            d.pos = if d.vertical { q.x } else { q.y };
            (d.vertical, d.index, d.page)
        });
        let pos = if vertical { q.x } else { q.y };
        let inside = (0.0..=if vertical { ps.x } else { ps.y }).contains(&pos) && (page == from || index.is_none());
        let r = screen_rect(app, from);
        let g = Guide { vertical, pos };
        if inside {
            painter.line_segment(line(r, &g), Stroke::new(1.0, COLOR));
            // the position, as Pages shows it
            let at = if vertical { pos2(r.min.x + pos * zoom, p.y) } else { pos2(p.x, r.min.y + pos * zoom) } + vec2(8.0, 8.0);
            let galley = painter.layout_no_wrap(crate::ruler::fmt_len(pos, app.settings.ruler_units), egui::FontId::proportional(12.0), Color32::WHITE);
            let lr = Rect::from_min_size(at, galley.size() + vec2(12.0, 6.0));
            painter.rect_filled(lr, 4.0, Color32::from_black_alpha(200));
            painter.galley(lr.min + vec2(6.0, 3.0), galley, Color32::WHITE);
        }
        ui.ctx().set_cursor_icon(if vertical { egui::CursorIcon::ResizeHorizontal } else { egui::CursorIcon::ResizeVertical });
        if !down {
            with(|s| s.drag = None);
            let mut gs = on_page_own(app, from);
            match index {
                Some(i) if i < gs.len() => {
                    if inside {
                        gs[i] = g;
                    } else {
                        gs.remove(i);
                    }
                }
                _ if inside => gs.push(g),
                _ => return true,
            }
            set(app, from, &gs, if index.is_some() && !inside { "Remove Guide" } else if index.is_some() { "Move Guide" } else { "Add Guide" });
        }
        return true;
    }

    // grab a guide (not over an object, not while a shape's points or a table are in use)
    if !with(|s| s.shown) {
        return false;
    }
    let tol = 4.0;
    let near = |p: Pos2| {
        lines.iter().find(|(pi, _, g)| {
            let r = screen_rect(app, *pi);
            r.expand(2.0).contains(p) && if g.vertical { (p.x - (r.min.x + g.pos * zoom)).abs() < tol } else { (p.y - (r.min.y + g.pos * zoom)).abs() < tol }
        })
    };
    let over_object = |p: Pos2| {
        let l = app.layout.as_ref().unwrap();
        app.canvas.hit_page(l, p).is_some_and(|(pi, q)| {
            app.drawables_on(pi).any(|d| Rect::from_min_size(pos2(d.geom.x, d.geom.y), vec2(d.geom.w.max(1.0), d.geom.h.max(1.0))).expand(2.0).contains(q))
        })
    };
    let Some(p) = pointer.filter(|p| view.contains(*p)) else { return false };
    let Some(&(pi, i, g)) = near(p).filter(|_| !over_object(p) && !app.canvas.dragging()) else { return false };
    ui.ctx().set_cursor_icon(if g.vertical { egui::CursorIcon::ResizeHorizontal } else { egui::CursorIcon::ResizeVertical });
    if ui.input(|i| i.pointer.primary_pressed()) {
        with(|s| s.drag = Some(Drag { page: pi, index: Some(i), vertical: g.vertical, pos: g.pos }));
    }
    true
}

fn on_page_own(app: &App, pi: usize) -> Vec<Guide> {
    match (app.wb.as_ref(), app.layout.as_ref()) {
        (Some(w), Some(l)) => w.page_guides(l, pi),
        _ => Vec::new(),
    }
}

fn set(app: &mut App, pi: usize, gs: &[Guide], label: &str) {
    let Some(l) = app.layout.take() else { return };
    app.edit_pages(label, |p, h| p.set_page_guides(h, &l, pi, gs));
    with(|s| s.cache.0 = u64::MAX);
    if app.layout.is_none() {
        app.layout = Some(l);
    }
}

/// The context menu's guide items (for the page that was right-clicked).
pub fn menu_items(app: &mut App, ui: &mut egui::Ui) {
    let Some((pi, q)) = with(|s| s.menu_at) else { return };
    ui.separator();
    for (label, vertical) in [("Add Horizontal Guide", false), ("Add Vertical Guide", true)] {
        if ui.button(label).clicked() {
            ui.close();
            let mut gs = on_page_own(app, pi);
            gs.push(Guide { vertical, pos: if vertical { q.x } else { q.y } });
            with(|s| s.shown = true);
            set(app, pi, &gs, "Add Guide");
        }
    }
}

/// View > Guides commands.
pub fn command(app: &mut App, name: &str) -> bool {
    match name {
        "Guides > Hide Guides" => with(|s| s.shown = !s.shown),
        "Guides > Hide Page Template Guides" => with(|s| s.template_shown = !s.template_shown),
        "Guides > Clear All Guides on Page" => {
            let pi = app.current_page();
            set(app, pi, &[], "Clear Guides");
        }
        _ => return false,
    }
    true
}

pub fn has_guides_on_current_page(app: &App) -> bool {
    !on_page_own(app, app.current_page()).is_empty()
}
