//! Values as data, for front ends that draw everything themselves (the Flutter app, ffi.rs):
//! the sheet's view (where it is, its tiles), the pointer on it (canvas.rs's clicks and drags
//! without an egui pass), what is drawn over it, the selected table's chrome, the menus, the
//! toolbar, the sheet tabs and the cell bar. Sheet points throughout unless it says view points
//! (from the view's top-left, the front end's points).

use std::cell::RefCell;

use egui::{pos2, vec2, Pos2, Rect, Vec2};
use nm_core::kn_format::ObjId;
use nm_core::kn_model::tablerows::{Axis, HeaderKind};
use nm_core::kn_model::DrawableKind;
use serde_json::{json, Value};

use crate::app::{App, InspTab, Sel};
use crate::canvas::{self, commit, Drag, Grid};
use crate::tiles;

fn col(c: nm_core::kn_model::Color) -> Value {
    json!([c.r, c.g, c.b, c.a])
}

// ------------------------------------------------------------------ the view

/// The sheet's content (sheet points): the frames of everything on it.
fn content(app: &App) -> (Pos2, Pos2) {
    let (mut lo, mut hi) = (pos2(0.0, 0.0), pos2(0.0, 0.0));
    for d in &app.drawables {
        let g = tiles::frame(d);
        lo = pos2(lo.x.min(g.x), lo.y.min(g.y));
        hi = pos2(hi.x.max(g.x + g.w), hi.y.max(g.y + g.h));
    }
    (lo, hi)
}

/// Room left and above the content (view points): Numbers' 40-point margin, more above for
/// the touch chrome's column bar over a table's name.
const TOUCH_MARGIN: (f32, f32) = (44.0, 96.0);

/// Where the view may go at zoom `z`: its top-left from the content's top-left less Numbers'
/// 40-point margin to its far edge plus ~350 / ~311 points less the view (canvas.rs).
fn bounds(app: &App, z: f32) -> (Pos2, Pos2) {
    let (lo, hi) = content(app);
    let (vw, vh) = (app.canvas.size.x / z, app.canvas.size.y / z);
    // (a finger's room: the table chrome's bars and a table's name above and left of the content)
    let min = pos2(lo.x - TOUCH_MARGIN.0 / z, lo.y - TOUCH_MARGIN.1 / z);
    let max = pos2((hi.x + 350.0 / z - vw).max(min.x), (hi.y + 311.0 / z - vh).max(min.y));
    (min, max)
}

/// After anything: the sheet's drawables current, its view placed (first time: where the
/// document was saved showing it, else the content's top-left with the margin), a zoom step
/// or a part of the sheet asked for (menus, Find) done at once, the view kept in bounds.
pub fn settle(app: &mut App) {
    app.refresh();
    if let Some(id) = app.sheets().get(app.sheet).map(|s| s.id) {
        app.canvas.tiles.show_sheet(id);
    }
    if app.canvas.origin.is_none() {
        match app.saved_view() {
            Some((z, o)) => {
                app.canvas.zoom = z;
                app.canvas.origin = Some(o);
            }
            None => {
                app.canvas.zoom = app.settings.default_zoom;
                let z = app.canvas.zoom;
                let (lo, _) = content(app);
                app.canvas.origin = Some(pos2(lo.x - TOUCH_MARGIN.0 / z, lo.y - TOUCH_MARGIN.1 / z));
            }
        }
    }
    let mut o = app.canvas.origin.unwrap_or_default();
    if let Some((_, z1, to, ..)) = app.canvas.zoom_anim.take() {
        let half = app.canvas.size / 2.0;
        let c = o + half / app.canvas.zoom;
        app.canvas.zoom = z1;
        o = match to {
            Some(end) => end - half / z1,
            None => c - half / z1,
        };
    }
    let zoom = app.canvas.zoom;
    if let Some(r) = app.canvas.reveal.take() {
        let vis = Rect::from_min_size(o, app.canvas.size / zoom);
        let margin = vec2(40.0, 60.0) / zoom;
        if r.min.x < vis.min.x + margin.x || r.max.x > vis.max.x - margin.x {
            o.x = r.min.x - margin.x * 2.0;
        }
        if r.min.y < vis.min.y + margin.y || r.max.y > vis.max.y - margin.y {
            o.y = r.min.y - vis.height() / 3.0;
        }
    }
    // (not while something is dragged out past the content: the sheet grows with it)
    if TOUCH.with(|t| t.borrow().press.is_none()) {
        let (min, max) = bounds(app, zoom);
        o = pos2(o.x.clamp(min.x, max.x), o.y.clamp(min.y, max.y));
    }
    app.canvas.origin = Some(o);
    if let Some(id) = app.sheets().get(app.sheet).map(|s| s.id) {
        app.session_views.insert(id, (zoom, o));
    }
}

/// The view is `w` × `h` points.
pub fn set_view(app: &mut App, w: f32, h: f32) {
    app.canvas.size = vec2(w.max(1.0), h.max(1.0));
}

/// The front end scrolled or zoomed: the sheet point at the view's top-left, the zoom.
pub fn set_scroll(app: &mut App, x: f32, y: f32, zoom: f32) {
    app.canvas.origin = Some(pos2(x, y));
    app.canvas.zoom = zoom.clamp(0.25, 4.0);
    app.canvas.zoom_anim = None;
}

/// {zoom, scroll, content: [x0, y0, x1, y1], version, sheet, tile (pixels)}.
pub fn geometry(app: &App) -> Value {
    let (lo, hi) = content(app);
    let o = app.canvas.origin.unwrap_or_default();
    json!({
        "zoom": app.canvas.zoom,
        "scroll": [o.x, o.y],
        "content": [lo.x, lo.y, hi.x, hi.y],
        "version": app.version,
        "sheet": app.sheet,
        "tile": tiles::TILE,
    })
}

/// Tile (`tx`, `ty`) at `scale` pixels a point ([`tiles::TILE`] pixels square, RGBA premultiplied).
pub fn tile(app: &mut App, tx: i32, ty: i32, scale: f32) -> Option<tiny_skia::Pixmap> {
    let wb = app.wb.as_ref()?;
    let span = tiles::TILE as f32 / scale;
    Some(app.canvas.tiles.tile_pixmap(&wb.doc, &app.drawables, tx, ty, span, scale))
}

// ------------------------------------------------------------------ the pointer

fn to_sheet(app: &App, p: Pos2) -> Pos2 {
    app.canvas.origin.unwrap_or_default() + p.to_vec2() / app.canvas.zoom
}

fn to_view(app: &App, p: Pos2) -> Pos2 {
    pos2(0.0, 0.0) + (p - app.canvas.origin.unwrap_or_default()) * app.canvas.zoom
}

fn grids(app: &App) -> Vec<(ObjId, Grid)> {
    app.drawables
        .iter()
        .filter_map(|d| match &d.kind {
            DrawableKind::Table(t) => Some((d.id, Grid::new(d.geom, t))),
            _ => None,
        })
        .collect()
}

/// The table cell at sheet point `s` (the top-most table).
pub fn cell_at(app: &App, s: Pos2) -> Option<(ObjId, u32, u32)> {
    grids(app).iter().rev().find_map(|(id, g)| Some((*id, g.row_at(s.y)?, g.col_at(s.x)?)))
}

/// The top-most object at sheet point `s` that isn't a table.
fn object_at(app: &App, s: Pos2) -> Option<ObjId> {
    let (id, table) = app.drawables.iter().rev().map(|d| (d.id, tiles::frame(d), matches!(d.kind, DrawableKind::Table(_)))).find(|(_, g, _)| s.x >= g.x && s.x <= g.x + g.w.max(8.0) && s.y >= g.y && s.y <= g.y + g.h.max(8.0)).map(|(id, _, t)| (id, t))?;
    (!table).then_some(id)
}

fn obj_rect(app: &App, id: ObjId) -> Option<Rect> {
    let d = app.drawables.iter().find(|d| d.id == id)?;
    let g = tiles::frame(d);
    Some(Rect::from_min_size(pos2(g.x, g.y), vec2(g.w.max(8.0), g.h.max(8.0))))
}

/// A press going on.
struct Press {
    /// View point it went down at, and the selection before it (a tap on the selected cell edits it).
    at: Pos2,
    before: Option<Sel>,
    count: u8,
    moved: bool,
    shift: bool,
    /// It put a cell reference into the formula being typed.
    picked: bool,
    /// It landed in the text box being edited.
    text: bool,
}

/// Feedback while dragging an object: its frame now, the guides it snapped to, the tip.
#[derive(Clone)]
struct Live {
    frame: Rect,
    guides: Vec<[f32; 4]>,
    tip: String,
}

#[derive(Default)]
struct Touch {
    press: Option<Press>,
    /// The last press (seconds, view point, count): another soon after near it is a double click.
    last: Option<(f64, Pos2, u8)>,
    live: Option<Live>,
    /// The pointer now (view point).
    now: Pos2,
}

thread_local! {
    static TOUCH: RefCell<Touch> = RefCell::new(Touch::default());
}

fn guide_lines(guides: &[kn_ui::snap::Guide], zoom: f32) -> Vec<[f32; 4]> {
    use kn_ui::snap::Guide;
    let mut out = Vec::new();
    for g in guides {
        match g {
            Guide::Line(a, b) => out.push([a.x, a.y, b.x, b.y]),
            Guide::Gap { axis, from, to, at } => {
                if *axis == 0 {
                    out.push([*from, *at, *to, *at]);
                } else {
                    out.push([*at, *from, *at, *to]);
                }
            }
            Guide::Size { axis, bx } => {
                let k = 8.0 / zoom;
                if *axis == 0 {
                    out.push([bx.x0, bx.y0 - k, bx.x1, bx.y0 - k]);
                } else {
                    out.push([bx.x0 - k, bx.y0, bx.x0 - k, bx.y1]);
                }
            }
        }
    }
    out
}

/// The pointer on the view: `kind` 0 moved (held), 1 pressed, 2 released; `secondary` a
/// right-click (or a touch and hold): what's there is selected by the desktop's rules.
pub fn pointer(app: &mut App, kind: u32, p: Pos2, secondary: bool, shift: bool, time: f64) {
    TOUCH.with(|t| t.borrow_mut().now = p);
    match kind {
        1 if secondary => press_secondary(app, p),
        1 => press(app, p, shift, time),
        0 => drag(app, p),
        _ => release(app, p),
    }
}

fn press_secondary(app: &mut App, p: Pos2) {
    let s = to_sheet(app, p);
    if let Some(id) = object_at(app, s) {
        if !app.selected_objs().contains(&id) {
            commit(app);
            app.sel = None;
            app.canvas.multi.clear();
            app.canvas.obj = Some(id);
        }
    } else if let Some((id, r, c)) = cell_at(app, s) {
        let inside = app.sel.filter(|x| x.table == id).is_some_and(|x| {
            let b = app.with_merges(id, x.block());
            r >= b.0 && r <= b.2 && c >= b.1 && c <= b.3
        });
        if !inside {
            commit(app);
            let (r, c) = app.merge_origin(id, r, c);
            app.canvas.obj = None;
            app.sel = Some(Sel::cell(id, r, c));
        }
    }
}

fn press(app: &mut App, p: Pos2, shift: bool, time: f64) {
    let s = to_sheet(app, p);
    let count = TOUCH.with(|t| match t.borrow().last {
        Some((t0, q, n)) if time - t0 < 0.45 && q.distance(p) < 8.0 => n + 1,
        _ => 1,
    });
    TOUCH.with(|t| t.borrow_mut().last = Some((time, p, count)));
    let mut pr = Press { at: p, before: app.sel, count, moved: false, shift, picked: false, text: false };
    app.canvas.drag = None;
    // the selected object's handles (a finger's reach)
    let handle = app.canvas.obj.filter(|o| app.table(*o).is_none()).and_then(|o| {
        let r = obj_rect(app, o)?;
        let pts = canvas::handle_points(Rect::from_min_max(to_view(app, r.min), to_view(app, r.max)));
        pts.iter().position(|h| h.distance(p) <= 16.0).map(|k| (o, k))
    });
    if let Some((id, handle)) = handle {
        commit(app);
        app.canvas.drag = Some(Drag::ObjSize { id, handle, from: p });
        TOUCH.with(|t| t.borrow_mut().press = Some(pr));
        return;
    }
    // typing a formula: a cell of the same table puts its reference in
    let formula = app.canvas.edit.as_ref().filter(|e| e.text.trim_start().starts_with('=')).map(|e| (e.table, e.row, e.col));
    if let (Some((t, er, ec)), Some((id, r, c))) = (formula, cell_at(app, s)) {
        if let (true, Some(mr)) = (id == t && (r, c) != (er, ec), app.mrow(id, r)) {
            let pick = app.canvas.pick.clone();
            if let Some(e) = app.canvas.edit.as_mut() {
                let base = match &pick {
                    Some((base, ins, _)) if e.text == format!("{base}{ins}") => base.clone(),
                    _ => e.text.clone(),
                };
                let ins = format!("{}{}", canvas::col_name(c), mr + 1);
                e.text = format!("{base}{ins}");
                e.focus = true;
                app.canvas.pick = Some((base, ins, (r, c)));
            }
            pr.picked = true;
            TOUCH.with(|t| t.borrow_mut().press = Some(pr));
            return;
        }
    }
    // the text box being edited: the caret goes there (Shift extends); outside, editing ends
    if let Some(id) = app.canvas.editing.as_ref().map(|e| e.id) {
        if let Some((tl, g)) = crate::textedit::layout_of(app, id) {
            let pt = nm_core::kn_model::Point::new(s.x, s.y);
            if crate::textedit::contains(&g, pt) {
                crate::textedit::point_to(app, &tl, &g, pt, shift);
                if count >= 2 {
                    select_word(app);
                }
                pr.text = true;
                TOUCH.with(|t| t.borrow_mut().press = Some(pr));
                return;
            }
            crate::textedit::commit(app);
        }
    }
    if let Some(id) = object_at(app, s) {
        commit(app);
        app.sel = None;
        match app.canvas.obj {
            Some(first) if shift && first != id => {
                if app.canvas.multi.first() != Some(&first) {
                    app.canvas.multi = vec![first];
                }
                match app.canvas.multi.iter().position(|x| *x == id) {
                    Some(i) => {
                        app.canvas.multi.remove(i);
                    }
                    None => app.canvas.multi.push(id),
                }
            }
            Some(_) if app.selected_objs().contains(&id) => {}
            _ => app.canvas.multi.clear(),
        }
        if !shift && !app.selected_objs().contains(&id) || app.canvas.obj.is_none() {
            app.canvas.obj = Some(id);
        }
        if app.insp_tab != InspTab::Organize {
            app.insp_tab = InspTab::Arrange;
        }
        // a double tap: its text, the caret where it was tapped
        if count >= 2 && crate::textedit::begin(app, id, Some(nm_core::kn_model::Point::new(s.x, s.y))) {
            pr.text = true;
        } else {
            app.canvas.drag = Some(Drag::Move { id, from: p });
        }
        TOUCH.with(|t| t.borrow_mut().press = Some(pr));
        return;
    }
    if let Some((id, r, c)) = cell_at(app, s) {
        app.canvas.obj = None;
        app.canvas.multi.clear();
        let (r, c) = app.merge_origin(id, r, c);
        let editing_here = app.canvas.edit.as_ref().is_some_and(|e| e.table == id && e.row == r && e.col == c);
        if !editing_here {
            commit(app);
            app.canvas.return_col = None;
            match app.sel {
                Some(x) if shift && x.table == id => app.sel = Some(Sel { cursor: (r, c), ..x }),
                _ => app.sel = Some(Sel::cell(id, r, c)),
            }
            if app.insp_tab == InspTab::Arrange {
                app.insp_tab = InspTab::Cell;
            }
            app.canvas.drag = Some(Drag::Cells(id));
            if count >= 2 && !shift {
                canvas::start_edit(app, None);
            }
        }
        TOUCH.with(|t| t.borrow_mut().press = Some(pr));
        return;
    }
    commit(app);
    app.sel = None;
    app.canvas.obj = None;
    app.canvas.multi.clear();
    TOUCH.with(|t| t.borrow_mut().press = Some(pr));
}

/// A double tap in text being edited: the word there.
fn select_word(app: &mut App) {
    let Some(text) = app.canvas.editing.as_ref().and_then(|e| app.doc().map(|d| d.storage_text(e.storage))) else { return };
    let Some(ed) = app.canvas.editing.as_mut() else { return };
    let i = ed.cursor.min(text.len());
    let word = |c: char| c.is_alphanumeric() || c == '\'' || c == '_';
    let a = text[..i].char_indices().rev().take_while(|(_, c)| word(*c)).last().map(|(k, _)| k).unwrap_or(i);
    let b = text[i..].char_indices().find(|(_, c)| !word(*c)).map(|(k, _)| i + k).unwrap_or(text.len());
    ed.anchor = a;
    ed.cursor = b;
}

fn drag(app: &mut App, p: Pos2) {
    let Some((from, picked, text, shift)) = TOUCH.with(|t| t.borrow().press.as_ref().map(|x| (x.at, x.picked, x.text, x.shift))) else { return };
    if p.distance(from) > 3.0 {
        TOUCH.with(|t| {
            if let Some(x) = t.borrow_mut().press.as_mut() {
                x.moved = true;
            }
        });
    }
    let s = to_sheet(app, p);
    let zoom = app.canvas.zoom;
    if text {
        if let Some(id) = app.canvas.editing.as_ref().map(|e| e.id) {
            if let Some((tl, g)) = crate::textedit::layout_of(app, id) {
                crate::textedit::point_to(app, &tl, &g, nm_core::kn_model::Point::new(s.x, s.y), true);
            }
        }
        return;
    }
    if picked {
        // a range: from the cell the press picked to the one under the finger
        let (Some((base, _, from_cell)), Some(e)) = (app.canvas.pick.clone(), app.canvas.edit.as_ref()) else { return };
        let Some((id, r, c)) = cell_at(app, s) else { return };
        if id != e.table {
            return;
        }
        let fr = app.mrow(id, from_cell.0).unwrap_or(from_cell.0);
        let tr = app.mrow(id, r).unwrap_or(r);
        let (r0, c0, r1, c1) = (fr.min(tr), from_cell.1.min(c), fr.max(tr), from_cell.1.max(c));
        let n = canvas::col_name;
        let ins = if (r0, c0) == (r1, c1) { format!("{}{}", n(c0), r0 + 1) } else { format!("{}{}:{}{}", n(c0), r0 + 1, n(c1), r1 + 1) };
        if let Some(e) = app.canvas.edit.as_mut() {
            e.text = format!("{base}{ins}");
        }
        app.canvas.pick = Some((base, ins, from_cell));
        return;
    }
    let others = |app: &App, id: ObjId| -> Vec<kn_ui::snap::Bx> {
        app.drawables
            .iter()
            .filter(|o| o.id != id)
            .map(|o| {
                let f = tiles::frame(o);
                kn_ui::snap::Bx { x0: f.x, y0: f.y, x1: f.x + f.w, y1: f.y + f.h }
            })
            .collect()
    };
    match app.canvas.drag {
        Some(Drag::Cells(t)) => {
            let Some(sel) = app.sel.filter(|x| x.table == t) else { return };
            let Some((_, g)) = grids(app).into_iter().find(|g| g.0 == t) else { return };
            let r = g.row_at(s.y).unwrap_or(if s.y < g.g.y { 0 } else { g.ys.len() as u32 - 2 });
            let c = g.col_at(s.x).unwrap_or(if s.x < g.g.x { 0 } else { g.xs.len() as u32 - 2 });
            app.sel = Some(Sel { cursor: (r, c), ..sel });
        }
        Some(Drag::Move { id, from }) => {
            let Some(d) = app.drawables.iter().find(|d| d.id == id) else { return };
            let g = tiles::frame(d);
            let mut delta = (p - from) / zoom;
            if shift {
                let len = delta.length();
                let a = (delta.y.atan2(delta.x) / std::f32::consts::FRAC_PI_4).round() * std::f32::consts::FRAC_PI_4;
                let along = delta.x * a.cos() + delta.y * a.sin();
                if len > 0.0 {
                    delta = vec2(along * a.cos(), along * a.sin());
                }
            }
            let bx = kn_ui::snap::Bx { x0: g.x, y0: g.y, x1: g.x + g.w, y1: g.y + g.h };
            let st = &app.settings;
            let opts = kn_ui::snap::Opts { center: st.guides_center, edges: st.guides_edges, spacing: st.guides_spacing, sizing: st.guides_sizing, tol: 6.0 / zoom };
            let page = kn_ui::snap::Bx { x0: 0.0, y0: 0.0, x1: 0.0, y1: 0.0 };
            let (d2, guides) = kn_ui::snap::snap_move(bx, &others(app, id), page, (delta.x, delta.y), &opts);
            let frame = Rect::from_min_size(pos2(g.x + d2.0, g.y + d2.1), vec2(g.w.max(8.0), g.h.max(8.0)));
            let tip = if app.settings.show_size_while_moving { format!("x: {:.0} pt  y: {:.0} pt", frame.min.x, frame.min.y) } else { String::new() };
            let live = Live { frame, guides: guide_lines(&guides, zoom), tip };
            TOUCH.with(|t| t.borrow_mut().live = Some(live));
        }
        Some(Drag::ObjSize { id, handle, from }) => {
            let Some(d) = app.drawables.iter().find(|d| d.id == id) else { return };
            let g = tiles::frame(d);
            let delta = (p - from) / zoom;
            let (l, t, r, b) = canvas::HANDLE_EDGES[handle];
            let mut bx = kn_ui::snap::Bx { x0: g.x + if l { delta.x } else { 0.0 }, y0: g.y + if t { delta.y } else { 0.0 }, x1: g.x + g.w + if r { delta.x } else { 0.0 }, y1: g.y + g.h + if b { delta.y } else { 0.0 } };
            if shift && (l || r) && (t || b) && g.w > 0.0 && g.h > 0.0 {
                let k = ((bx.x1 - bx.x0) / g.w).abs().max(((bx.y1 - bx.y0) / g.h).abs());
                let (w, h) = (g.w * k, g.h * k);
                if l { bx.x0 = bx.x1 - w } else { bx.x1 = bx.x0 + w }
                if t { bx.y0 = bx.y1 - h } else { bx.y1 = bx.y0 + h }
            }
            let st = &app.settings;
            let opts = kn_ui::snap::Opts { center: st.guides_center, edges: st.guides_edges, spacing: false, sizing: st.guides_sizing, tol: 6.0 / zoom };
            let page = kn_ui::snap::Bx { x0: 0.0, y0: 0.0, x1: 0.0, y1: 0.0 };
            let (bx, guides) = if shift { (bx, Vec::new()) } else { kn_ui::snap::snap_resize(bx, (l, t, r, b), &others(app, id), page, &opts) };
            let frame = Rect::from_min_max(pos2(bx.x0.min(bx.x1), bx.y0.min(bx.y1)), pos2(bx.x0.max(bx.x1), bx.y0.max(bx.y1)));
            let tip = if app.settings.show_size_while_moving { format!("w: {:.0} pt  h: {:.0} pt", frame.width(), frame.height()) } else { String::new() };
            TOUCH.with(|t| t.borrow_mut().live = Some(Live { frame, guides: guide_lines(&guides, zoom), tip }));
        }
        _ => {}
    }
}

fn release(app: &mut App, p: Pos2) {
    let Some(pr) = TOUCH.with(|t| t.borrow_mut().press.take()) else { return };
    let live = TOUCH.with(|t| t.borrow_mut().live.take());
    let drag = app.canvas.drag.take();
    if pr.picked || pr.text {
        return;
    }
    match drag {
        Some(Drag::Move { id, .. }) => {
            if let (Some(l), Some(d)) = (live, app.drawables.iter().find(|d| d.id == id)) {
                let g = tiles::frame(d);
                let (dx, dy) = (l.frame.min.x - g.x, l.frame.min.y - g.y);
                if dx != 0.0 || dy != 0.0 {
                    let mut ids = app.selected_objs();
                    if !ids.contains(&id) {
                        ids = vec![id];
                    }
                    if app.mask_edit == Some(id) {
                        crate::mask::move_under_mask(app, id, dx, dy);
                    } else {
                        let all: Vec<ObjId> = app.drawables.iter().map(|d| d.id).collect();
                        app.edit("Move", |doc, h| {
                            for i in &ids {
                                doc.move_by(h, *i, dx, dy);
                            }
                            doc.update_connections_in(h, &all, &ids);
                        });
                    }
                }
            }
        }
        Some(Drag::ObjSize { id, handle, .. }) => {
            if let (Some(l), Some(d)) = (live, app.drawables.iter().find(|d| d.id == id)) {
                let (lf, t, r, b) = canvas::HANDLE_EDGES[handle];
                let mut ng = d.geom;
                ng.x = l.frame.min.x;
                ng.y = l.frame.min.y;
                ng.w = l.frame.width().max(4.0);
                ng.h = l.frame.height().max(4.0);
                if lf || r {
                    ng.auto_width = false;
                }
                if t || b {
                    ng.auto_height = false;
                }
                if ng != d.geom {
                    let all: Vec<ObjId> = app.drawables.iter().map(|d| d.id).collect();
                    app.edit("Resize", |doc, h| {
                        doc.set_frame(h, id, ng);
                        doc.update_connections_in(h, &all, &[id]);
                    });
                }
            }
        }
        Some(Drag::Cells(t)) if !pr.moved && !pr.shift && pr.count == 1 => {
            // a tap on the cell that already was the selection edits it (Numbers); a tap on a
            // checkbox or star rating changes it
            let s = to_sheet(app, p);
            let Some(sel) = app.sel.filter(|x| x.table == t) else { return };
            if pr.before == Some(sel) && sel.anchor == sel.cursor {
                canvas::start_edit(app, None);
            } else if let Some((id, r, c)) = cell_at(app, s).filter(|x| x.0 == t) {
                let x0 = grids(app).iter().find(|g| g.0 == id).map(|(_, g)| g.g.x + g.xs[c as usize]).unwrap_or(0.0);
                canvas::click_control(app, id, r, c, s.x - x0);
            }
        }
        _ => {}
    }
}

/// The table being moved by its corner (the chrome's): the drag follows the pointer.
pub fn start_table_move(app: &mut App, id: ObjId, p: Pos2) {
    commit(app);
    app.sel = None;
    app.canvas.obj = None;
    TOUCH.with(|t| t.borrow_mut().press = Some(Press { at: p, before: None, count: 1, moved: false, shift: false, picked: false, text: false }));
    app.canvas.drag = Some(Drag::Move { id, from: p });
}

// ------------------------------------------------------------------ what's over the sheet

/// Drawn over the sheet: the selected objects (frame; handles on the first), a drag's live
/// frame, guides and tip, the text being edited (caret, selection, its frame), commented cells'
/// corners, the cells a formula being typed refers to, Edit Data References' outlines, table
/// titles (a double tap renames). Sheet points.
pub fn overlays(app: &mut App) -> Value {
    let r4 = |r: Rect| json!([r.min.x, r.min.y, r.max.x, r.max.y]);
    let mut objects = Vec::new();
    let editing = app.canvas.editing.as_ref().map(|e| e.id);
    for (k, id) in app.selected_objs().into_iter().enumerate() {
        if app.table(id).is_some() {
            continue;
        }
        let Some(r) = obj_rect(app, id) else { continue };
        let locked = app.drawables.iter().find(|d| d.id == id).is_some_and(|d| d.locked);
        objects.push(json!({ "id": id, "frame": r4(r), "handles": k == 0 && editing != Some(id) && app.points.id != Some(id), "editing": editing == Some(id), "locked": locked }));
    }
    let live = TOUCH.with(|t| t.borrow().live.clone()).map(|l| json!({ "frame": r4(l.frame), "guides": l.guides, "tip": l.tip }));
    // the text being edited
    let mut text = Value::Null;
    if let Some(id) = editing {
        if let Some((tl, g)) = crate::textedit::layout_of(app, id) {
            let mut g2 = g;
            g2.hflip = false;
            g2.vflip = false;
            let map = |x: f32, y: f32| {
                let q = g2.to_slide(nm_core::kn_model::Point::new(x, y));
                [q.x, q.y]
            };
            let ed = app.canvas.editing.as_ref().unwrap();
            let (a, b) = ed.sel();
            let mut rects = Vec::new();
            if a < b {
                for line in &tl.lines {
                    if line.end < a || line.start > b || (line.start == line.end && !(a..b).contains(&line.start)) {
                        continue;
                    }
                    let x0 = kn_render::text::caret(&tl, a.max(line.start)).filter(|_| a >= line.start).map(|c| c.0).unwrap_or(line.x);
                    let x1 = if b <= line.end { kn_render::text::caret(&tl, b).map(|c| c.0).unwrap_or(line.x + line.width) } else { line.x + line.width + 6.0 };
                    rects.push(json!([map(x0, line.top), map(x1, line.top), map(x1, line.top + line.height), map(x0, line.top + line.height)]));
                }
            }
            let caret = kn_render::text::caret(&tl, ed.cursor).map(|(x, top, h)| json!([map(x, top), map(x, top + h)]));
            let ends = match (kn_render::text::caret(&tl, a), kn_render::text::caret(&tl, b)) {
                (Some((xa, ta, ha)), Some((xb, tb, hb))) if a < b => json!([[map(xa, ta), map(xa, ta + ha)], [map(xb, tb), map(xb, tb + hb)]]),
                _ => Value::Null,
            };
            let frame = [map(0.0, 0.0), map(g.w, 0.0), map(g.w, g.h), map(0.0, g.h)];
            text = json!({ "id": id, "rects": rects, "caret": if a == b { caret } else { None }, "ends": ends, "frame": frame });
        }
    }
    let gs = grids(app);
    // commented cells' corners: your own in your Author Color, others' in yellow
    let mut comments = Vec::new();
    if app.show_comments {
        for (id, g) in &gs {
            let Some(cm) = app.comments.get(id) else { continue };
            for ((mr, c), (_, author)) in cm {
                let r = app.vrow(*id, *mr);
                if (r as usize) + 1 >= g.ys.len() || (*c as usize) + 1 >= g.xs.len() {
                    continue;
                }
                let rgb = if !author.is_empty() && *author == app.settings.author { crate::settings::AUTHOR_COLORS[app.settings.author_color as usize % 10].1 } else { 0xf5c518 };
                let b = g.block(r, *c, r, *c);
                comments.push(json!([b.min.x, b.min.y, b.max.x, b.max.y, rgb]));
            }
        }
    }
    // a formula being typed: the cells it refers to, each in its own colour
    let mut refs = Vec::new();
    if let Some(e) = app.canvas.edit.as_ref().filter(|e| e.text.trim_start().starts_with('=')) {
        if let Some((_, g)) = gs.iter().find(|g| g.0 == e.table) {
            for (i, (m0, c0, m1, c1)) in canvas::formula_refs(&e.text).into_iter().enumerate() {
                let (a, b) = (app.vrow(e.table, m0), app.vrow(e.table, m1));
                let (r0, r1) = (a.min(b), a.max(b));
                if (r1 as usize) < g.ys.len() - 1 && (c1 as usize) < g.xs.len() - 1 {
                    let k = canvas::REF_COLORS[i % canvas::REF_COLORS.len()];
                    let b = g.block(r0, c0, r1, c1);
                    refs.push(json!([b.min.x, b.min.y, b.max.x, b.max.y, (u32::from(k.r()) << 16) | (u32::from(k.g()) << 8) | u32::from(k.b())]));
                }
            }
        }
    }
    for (t, (r0, c0, r1, c1), k) in crate::datarefs::outlines(app) {
        if let Some((_, g)) = gs.iter().find(|g| g.0 == t) {
            if (r1 as usize) < g.ys.len() - 1 && (c1 as usize) < g.xs.len() - 1 {
                let b = g.block(r0, c0, r1, c1);
                refs.push(json!([b.min.x, b.min.y, b.max.x, b.max.y, (u32::from(k.r()) << 16) | (u32::from(k.g()) << 8) | u32::from(k.b())]));
            }
        }
    }
    // table titles, for renaming with a double tap
    let titles: Vec<Value> = gs
        .iter()
        .filter(|(id, _)| app.doc().is_some_and(|d| d.table_name_shown(*id)))
        .map(|(id, g)| json!({ "id": id, "x": g.g.x + g.w() / 2.0, "y": g.g.y - 6.0, "name": app.table(*id).map(|(_, t)| t.name.clone()).unwrap_or_default(), "editing": app.canvas.title_edit.as_ref().filter(|t| t.0 == *id).map(|t| t.1.clone()) }))
        .collect();
    // tables whose header rows / columns are frozen: kept in view while the table scrolls
    let frozen: Vec<Value> = app
        .drawables
        .iter()
        .filter_map(|d| {
            let DrawableKind::Table(t) = &d.kind else { return None };
            let flag = |k: &str| app.doc().and_then(|doc| doc.file.obj(d.id)?.ref_id("tableModel")).and_then(|m| app.doc()?.file.obj(m)?.bool(k)).unwrap_or(false);
            let (fr, fc) = (flag("header_rows_frozen") && t.header_rows > 0, flag("header_columns_frozen") && t.header_cols > 0);
            if !fr && !fc {
                return None;
            }
            let g = t.frame(&d.geom);
            Some(json!({
                "id": d.id, "rows": fr, "cols": fc,
                "frame": [g.x, g.y, g.w, g.h],
                "hh": t.row_heights.iter().take(t.header_rows as usize).sum::<f32>(),
                "hw": t.col_widths.iter().take(t.header_cols as usize).sum::<f32>(),
                "rowHeights": t.row_heights,
            }))
        })
        .collect();
    json!({
        "frozen": frozen,
        "objects": objects,
        "live": live,
        "text": text,
        "comments": comments,
        "refs": refs,
        "titles": titles,
        "dataRefs": app.data_refs.is_some(),
        "controls": controls(app),
        "version": app.version,
    })
}

/// What the desktop canvas draws over the sheet besides the selection (canvas.rs): the guides
/// and rulers, the pen bar, link badges, Edit Mask's bar, Edit Data References' bar, a shape's
/// points, movies' ▶ and a gallery's arrows, the cursor cell's comment. Sheet points.
fn controls(app: &mut App) -> Value {
    use nm_core::kn_model::DrawableKind;
    let fr = |g: nm_core::kn_model::Geometry| json!([g.x, g.y, g.x + g.w, g.y + g.h]);
    // guides (yours, as the desktop draws them while Hide Guides isn't on) and the rulers
    let sheet = app.sheets().get(app.sheet).map(|s| s.id);
    let lines: Vec<Value> = match (app.show_guides, sheet, app.wb.as_ref()) {
        (true, Some(s), Some(w)) => w.doc.guides(s).iter().map(|g| json!([g.vertical, g.pos])).collect(),
        _ => Vec::new(),
    };
    let guides = json!({ "lines": lines, "rulers": app.show_rulers, "units": app.settings.ruler_units });
    // Draw with Pen
    let pen = app.pen.as_ref().map(|_| {
        let c = app.pen_color;
        json!({ "color": [c.r(), c.g(), c.b()], "width": app.pen_width })
    });
    // objects with a link: the green badge (a tap follows it)
    let links: Vec<Value> = app
        .drawables
        .iter()
        .filter(|d| !matches!(d.kind, DrawableKind::Table(_)))
        .filter_map(|d| crate::links::object_link(app, d.id).map(|u| json!({ "id": d.id, "frame": fr(crate::tiles::frame(d)), "url": u })))
        .collect();
    // Edit Mask: the mask's frame and the picture's size against it (mask.rs mask_bar)
    let mask = app.mask_edit.and_then(|id| {
        let m = crate::mask::mask_box(app, id)?;
        let g = app.drawables.iter().find(|d| d.id == id)?.geom;
        Some(json!({ "id": id, "frame": fr(m), "pct": (g.w / m.w.max(0.01) * 100.0).clamp(100.0, 500.0) }))
    });
    let data_refs = app.data_refs.map(|m| json!({ "rows": m.rows_are_series }));
    // movies, sounds, web videos (their ▶) and galleries (their arrows and "2 of 5")
    let sel = app.canvas.obj;
    let mut media = Vec::new();
    let mut gallery = Value::Null;
    for d in &app.drawables {
        let g = crate::tiles::frame(d);
        match &d.kind {
            DrawableKind::Movie(m) => media.push(json!({ "id": d.id, "frame": fr(g), "audio": m.audio_only, "selected": sel == Some(d.id) })),
            DrawableKind::Image(im) if im.web_video.is_some() => media.push(json!({ "id": d.id, "frame": fr(g), "web": true, "selected": sel == Some(d.id) })),
            DrawableKind::Image(im) if im.gallery.len() > 1 && sel == Some(d.id) => {
                gallery = json!({ "id": d.id, "frame": fr(g), "n": im.gallery.len(), "shown": im.gallery_shown.min(im.gallery.len() - 1) });
            }
            _ => {}
        }
    }
    // the cursor cell's comment (the desktop shows it on hover)
    let comment = app.sel.filter(|s| s.anchor == s.cursor && app.canvas.edit.is_none()).and_then(|s| {
        let (text, author) = app.comments.get(&s.table)?.get(&(app.mrow(s.table, s.cursor.0)?, s.cursor.1))?.clone();
        Some(json!({ "text": text, "author": author }))
    });
    json!({
        "guides": guides, "pen": pen, "links": links, "mask": mask, "dataRefs": data_refs,
        "points": crate::points::api_state(app), "media": media, "gallery": gallery, "comment": comment,
    })
}

/// The canvas controls' actions ("ctl:<what>:…"): guides, the pen, links, the mask, data
/// references, points, movies, galleries.
fn control_action(app: &mut App, ctx: &egui::Context, a: &str) {
    let p: Vec<&str> = a.split(':').collect();
    let f = |k: usize| p.get(k).and_then(|v| v.parse::<f32>().ok());
    match p.first().copied().unwrap_or_default() {
        // "guide:<index, -1 new>:<v|h>:<pos, or x to remove>" (canvas.rs: a guide let go)
        "guide" => {
            let Some(s) = app.sheets().get(app.sheet).map(|s| s.id) else { return };
            let Some(mut gs) = app.wb.as_ref().map(|w| w.doc.guides(s)) else { return };
            let before = gs.clone();
            let index = p.get(1).and_then(|v| v.parse::<usize>().ok());
            let vertical = p.get(2) == Some(&"v");
            match (index, f(3)) {
                (Some(i), None) if i < gs.len() => {
                    gs.remove(i);
                }
                (Some(i), Some(pos)) if i < gs.len() => gs[i].pos = pos,
                (None, Some(pos)) => gs.push(nm_core::kn_model::slides::Guide { vertical, pos }),
                _ => {}
            }
            if gs != before {
                let removed = gs.len() < before.len();
                app.edit(if removed { "Remove Guide" } else { "Guide" }, |d, h| d.set_guides(h, s, &gs));
            }
        }
        // "pen:color:r:g:b", "pen:width:w", "pen:done", "pen:line:x,y;x,y;…" (a stroke drawn)
        "pen" => match p.get(1).copied().unwrap_or_default() {
            "color" => app.pen_color = egui::Color32::from_rgb(f(2).unwrap_or(0.0) as u8, f(3).unwrap_or(0.0) as u8, f(4).unwrap_or(0.0) as u8),
            "width" => app.pen_width = f(2).unwrap_or(2.0).clamp(1.0, 24.0),
            "line" => {
                let pts: Vec<Pos2> = p.get(2).unwrap_or(&"").split(';').filter_map(|q| q.split_once(',').and_then(|(x, y)| Some(pos2(x.parse().ok()?, y.parse().ok()?)))).collect();
                crate::canvas::pen_stroke(app, pts);
            }
            _ => app.pen = None,
        },
        "follow" => {
            if let Some(url) = p.get(1).and_then(|v| v.parse().ok()).and_then(|id| crate::links::object_link(app, id)) {
                crate::links::follow(app, &url);
            }
        }
        // "mask:size:<percent>", "mask:done"
        "mask" => match p.get(1).copied() {
            Some("size") => {
                if let Some(pct) = f(2) {
                    crate::mask::set_size(app, pct);
                }
            }
            _ => app.mask_edit = None,
        },
        // "datarefs:rows:<0|1>", "datarefs:done", "datarefs:cancel"
        "datarefs" => match p.get(1).copied() {
            Some("rows") => {
                if let Some(m) = app.data_refs.as_mut() {
                    m.rows_are_series = p.get(2) == Some(&"1");
                }
            }
            Some("done") => crate::datarefs::done(app),
            _ => {
                if let Some(m) = app.data_refs.take() {
                    crate::datarefs::select_chart(app, m.chart);
                }
            }
        },
        "points" => crate::points::action(app, &p[1..].join(":")),
        // "galleryshow:<id>:<k>"
        "galleryshow" => {
            if let (Some(id), Some(k)) = (p.get(1).and_then(|v| v.parse().ok()), p.get(2).and_then(|v| v.parse().ok())) {
                crate::inspapi::show_image(app, id, k);
            }
        }
        // "mediaopen:<id>": a movie or sound in the system's player, a web video in the browser
        "mediaopen" => {
            if let Some(id) = p.get(1).and_then(|v| v.parse().ok()) {
                crate::media::open_outside(app, ctx, id);
            }
        }
        _ => {}
    }
}

// ------------------------------------------------------------------ the selected table's chrome

/// The table with the selected cells, for its chrome (the Letters front end's table_chrome):
/// its place, sizes, the selection, the cell being edited, the row labels.
pub fn table_state(app: &mut App) -> Value {
    let Some(sel) = app.sel else { return Value::Null };
    let id = sel.table;
    let Some((d, t)) = app.table(id) else { return Value::Null };
    let g = t.frame(&d.geom);
    let (r0, c0, r1, c1) = app.with_merges(id, sel.block());
    let shown = app.doc().is_some_and(|doc| doc.table_name_shown(id));
    let labels: Vec<Value> = (0..t.rows).map(|r| app.mrow(id, r).map(|m| json!((m + 1).to_string())).unwrap_or(json!(""))).collect();
    let (rows, cols, hr, fr) = (t.rows, t.cols, t.header_rows, t.footer_rows);
    let (cw, rh, name) = (t.col_widths.clone(), t.row_heights.clone(), t.name.clone());
    let empty = |k: u32, rows_: bool| {
        let cell = |r: u32, c: u32| t.cells.get((r * t.cols + c) as usize).is_none_or(|x| x.trim().is_empty());
        if rows_ { (0..t.cols).all(|c| cell(k, c)) } else { (0..t.rows).all(|r| cell(r, k)) }
    };
    let floor = |rows_: bool, cur: u32| (1..cur).rev().take_while(|&k| empty(k, rows_)).last().unwrap_or(cur).max(1);
    let (row_floor, col_floor) = (floor(true, rows), floor(false, cols));
    let editing = editing_state(app);
    json!({
        "id": id,
        "page": 0,
        "x": g.x, "y": g.y,
        "colWidths": cw, "rowHeights": rh,
        "rows": rows, "cols": cols,
        "headerRows": hr, "footerRows": fr,
        "locked": false,
        "sel": [r0, c0, r1, c1],
        "anchor": [sel.anchor.0, sel.anchor.1],
        "cursor": [sel.cursor.0, sel.cursor.1],
        "editing": editing,
        "rowFloor": row_floor, "colFloor": col_floor,
        "rowLabels": labels,
        "name": name, "nameShown": shown,
        "categorized": app.categorized(id),
    })
}

/// The cell being edited: its text, the cell's look, the merge it spans, and what's offered
/// while typing (functions for a formula, the column's values for text).
fn editing_state(app: &mut App) -> Value {
    let Some(e) = app.canvas.edit.as_ref() else { return Value::Null };
    let (id, r, c, text) = (e.table, e.row, e.col, e.text.clone());
    let Some((_, t)) = app.table(id) else { return Value::Null };
    let style = t.cell_styles.get((r * t.cols + c) as usize).cloned().unwrap_or_default();
    let fill = match &style.fill {
        nm_core::kn_model::Fill::Color(k) if k.a > 0.99 => Some(col(*k)),
        nm_core::kn_model::Fill::Gradient { stops, .. } if !stops.is_empty() => Some(col(stops[0].1)),
        _ => None,
    };
    let formula = text.trim_start().starts_with('=');
    let align = match style.align {
        nm_core::kn_model::Align::Center => "center",
        nm_core::kn_model::Align::Right => "right",
        nm_core::kn_model::Align::Natural if !formula && text.trim().replace([',', '$', '%'], "").parse::<f64>().is_ok() => "right",
        _ => "left",
    };
    let b = app.with_merges(id, (r, c, r, c));
    let suggest: Vec<Value> = match canvas::function_token(&text).filter(|_| app.settings.formula_suggestions) {
        Some(tok) if !tok.is_empty() => {
            let up = tok.to_ascii_uppercase();
            let mut m: Vec<(&str, &str)> = nm_core::formula::functions().iter().filter(|(n, _)| n.starts_with(&up)).copied().collect();
            m.sort_by_key(|(n, _)| (n.len(), *n));
            m.truncate(8);
            m.iter().map(|(n, s)| json!([n, s])).collect()
        }
        _ => Vec::new(),
    };
    let offers: Vec<String> = if app.settings.autocomplete_cells && suggest.is_empty() { canvas::column_matches(app, id, r, c, &text) } else { Vec::new() };
    json!({
        "r": r, "c": c, "r1": b.2, "c1": b.3, "text": text,
        "size": if style.text.size > 0.0 { style.text.size } else { 13.0 },
        "color": (style.text.color.a > 0.0).then(|| col(style.text.color)),
        "bold": style.text.bold, "italic": style.text.italic,
        "fill": fill, "align": align, "padding": style.padding,
        "formula": formula,
        "suggest": suggest,
        "offers": offers,
    })
}

fn select_range(app: &mut App, id: ObjId, anchor: (u32, u32), cursor: (u32, u32)) {
    let Some((_, t)) = app.table(id) else { return };
    let cl = |(r, c): (u32, u32)| (r.min(t.rows.saturating_sub(1)), c.min(t.cols.saturating_sub(1)));
    let (anchor, cursor) = (cl(anchor), cl(cursor));
    app.sel = Some(Sel { table: id, anchor, cursor });
    app.canvas.obj = None;
    if app.insp_tab == InspTab::Arrange {
        app.insp_tab = InspTab::Cell;
    }
}

/// Grows or shrinks the table to `n` rows / columns (only empty ones at the end go).
fn set_count(app: &mut App, id: ObjId, rows: bool, n: u32) {
    let Some((r, c)) = app.table(id).map(|(_, t)| (t.rows, t.cols)) else { return };
    let (dr, dc) = if rows { (n as i32 - r as i32, 0) } else { (0, n as i32 - c as i32) };
    if dr != 0 || dc != 0 {
        canvas::grow_table(app, id, dr, dc);
    }
}

/// A table chrome action: "select:ar:ac:cr:cc" (anchor, cursor), "all", "edit:r:c",
/// "type:<text>", "text:<text>", "commit", "commitTab:<0|1 back>", "commitDown", "cancel",
/// "size:<rows 0|1>:i:pt", "count:<rows>:n", "move:lo:hi:before", "autofill:r1:c1",
/// "fit:<rows>:i", "clear", "moveStart:x:y" (the table follows the pointer), "fn:<NAME>" (a
/// function picked from the suggestions), "offer:<text>" (a value picked from the column's).
pub fn table_action(app: &mut App, a: &str) {
    let (kind, rest) = a.split_once(':').unwrap_or((a, ""));
    let Some(id) = app.sel.map(|s| s.table) else { return };
    let nums: Vec<u32> = rest.split(':').filter_map(|x| x.parse().ok()).collect();
    let n = |i: usize| nums.get(i).copied().unwrap_or(0);
    match kind {
        // the cells' context menu: Delete Table (canvas.rs table_menu)
        "deleteTable" => {
            commit(app);
            if let Some(sheet) = app.sheets().get(app.sheet).map(|s| s.id) {
                app.edit("Delete Table", |d, h| nm_core::sheets::detach_drawables(d, h, sheet, &[id]));
                app.sel = None;
                app.canvas.obj = None;
            }
        }
        "select" => {
            commit(app);
            select_range(app, id, (n(0), n(1)), (n(2), n(3)));
        }
        "all" => {
            commit(app);
            if let Some((_, t)) = app.table(id) {
                app.sel = Some(Sel { table: id, anchor: (0, 0), cursor: (t.rows.saturating_sub(1), t.cols.saturating_sub(1)) });
                app.insp_tab = InspTab::Table;
            }
        }
        "edit" => {
            commit(app);
            app.sel = Some(Sel::cell(id, n(0), n(1)));
            canvas::start_edit(app, None);
        }
        "type" => {
            if let Some(s) = app.sel {
                app.sel = Some(Sel::cell(id, s.cursor.0, s.cursor.1));
                canvas::start_edit(app, Some(rest.to_string()));
            }
        }
        "text" => {
            if let Some(e) = app.canvas.edit.as_mut() {
                let lines = |s: &str| s.matches('\n').count();
                let grew = lines(&e.text) != lines(rest);
                e.text = rest.to_string();
                // (the row grows with a new line)
                if grew {
                    app.version += 1;
                }
            }
        }
        "fn" => {
            if let Some(e) = app.canvas.edit.as_mut() {
                if let Some(tok) = canvas::function_token(&e.text).map(str::len) {
                    e.text.truncate(e.text.len() - tok);
                    e.text.push_str(rest);
                    e.text.push('(');
                }
            }
        }
        "offer" => {
            if let Some(e) = app.canvas.edit.as_mut() {
                e.text = rest.to_string();
            }
            commit(app);
        }
        "commit" => commit(app),
        "commitDown" => {
            commit(app);
            canvas::return_move(app);
        }
        "commitTab" => {
            commit(app);
            canvas::tab_move(app, rest == "1");
        }
        "cancel" => {
            app.canvas.edit = None;
            app.canvas.pick = None;
            app.version += 1;
        }
        "size" => {
            let rows = n(0) == 1;
            let size: f32 = rest.split(':').nth(2).and_then(|x| x.parse().ok()).unwrap_or(20.0);
            let i = n(1);
            commit(app);
            if app.edit(if rows { "Row Height" } else { "Column Width" }, |d, h| d.set_table_sizes(h, id, rows, &[(i, size.max(8.0))])) == Some(false) {
                app.notify("Couldn't change the size.");
            }
        }
        "count" => {
            commit(app);
            set_count(app, id, n(0) == 1, n(1).clamp(1, 1000));
        }
        "move" => {
            let (lo, hi, before) = (n(0), n(1), n(2));
            if app.categorized(id) {
                return app.notify("Rows wait until categories are off.");
            }
            commit(app);
            let cols = app.table(id).map(|(_, t)| t.cols).unwrap_or(1);
            match app.edit(if hi > lo { "Move Rows" } else { "Move Row" }, |d, h| d.move_table_rows(h, id, lo, hi, before)) {
                Some(Ok(at)) => select_range(app, id, (at, 0), (at + hi - lo, cols - 1)),
                Some(Err(e)) => app.notify(e.to_string()),
                None => {}
            }
        }
        "autofill" => {
            if let Some(s) = app.sel {
                let (r0, c0, r1, c1) = app.with_merges(id, s.block());
                let to = (r0, c0, n(0).max(r1), n(1).max(c1));
                if to != (r0, c0, r1, c1) {
                    commit(app);
                    if let Some(Err(e)) = app.edit("Autofill", |d, h| d.autofill(h, id, (r0, c0, r1, c1), to)) {
                        app.notify(format!("Autofill: {e}"));
                    }
                    app.sel = Some(Sel { table: id, anchor: (to.0, to.1), cursor: (to.2, to.3) });
                }
            }
        }
        "fit" => {
            let (rows, i) = (n(0) == 1, n(1));
            commit(app);
            if rows {
                app.edit("Fit Height to Content", |d, h| d.fit_table_to_content(h, id, true, &[i]));
            } else {
                let t = app.table(id).map(|(_, t)| t.clone());
                let w = t.and_then(|t| app.canvas.tiles.fit_width(&t, i));
                if let Some(w) = w {
                    app.edit("Fit Width to Content", |d, h| d.set_table_sizes(h, id, false, &[(i, w.max(20.0))]));
                }
            }
        }
        "clear" => crate::canvas::clear_cells(app),
        "moveStart" => {
            let xy: Vec<f32> = rest.split(':').filter_map(|v| v.parse().ok()).collect();
            if xy.len() == 2 {
                start_table_move(app, id, pos2(xy[0], xy[1]));
            }
        }
        _ => {}
    }
}

/// A row (`rows`) or column tab's menu.
pub fn tab_menu(app: &App, rows: bool, i: u32) -> Value {
    let item = |label: &str, cmd: &str| json!({ "label": label, "action": format!("cmd:{cmd}"), "enabled": crate::menus::enabled(app, cmd) });
    let sep = json!({ "separator": true });
    let mut v = Vec::new();
    if rows {
        v.extend([item("Add Row Above", "Table > Add Row Above"), item("Add Row Below", "Table > Add Row Below"), sep.clone(), item("Delete Row", "Table > Delete Row"), item("Hide Row", "Table > Hide Row")]);
    } else {
        v.extend([item("Sort Ascending", "Sort Ascending"), item("Sort Descending", "Sort Descending"), sep.clone(), item("Add Column Before", "Table > Add Column Before"), item("Add Column After", "Table > Add Column After"), sep.clone(), item("Delete Column", "Table > Delete Column"), item("Hide Column", "Table > Hide Column")]);
    }
    v.push(sep.clone());
    v.push(json!({ "label": if rows { "Fit Height to Content" } else { "Fit Width to Content" }, "action": format!("tbl:fit:{}:{i}", u8::from(rows)), "enabled": true }));
    v.push(sep);
    v.extend([item("Cut", "Cut"), item("Copy", "Copy"), item("Paste", "Paste"), item("Clear All", "Clear All")]);
    Value::Array(v)
}

// ------------------------------------------------------------------ menus

/// The edit menu (the desktop's right-click menu) for what's selected now.
pub fn context_menu(app: &App) -> Value {
    let item = |label: &str, cmd: &str| json!({ "label": label, "action": format!("cmd:{cmd}"), "enabled": crate::menus::enabled(app, cmd) });
    let sep = json!({ "separator": true });
    let mut v = Vec::new();
    if app.canvas.editing.is_some() {
        v.extend([item("Cut", "Cut"), item("Copy", "Copy"), item("Paste", "Paste"), item("Select All", "Select All")]);
        return Value::Array(v);
    }
    if !app.selected_objs().is_empty() {
        v.extend([item("Cut", "Cut"), item("Copy", "Copy"), item("Paste", "Paste"), item("Delete", "Delete"), item("Duplicate", "Duplicate Selection")]);
        v.push(sep.clone());
        v.extend([item("Bring to Front", "Bring to Front"), item("Send to Back", "Send to Back")]);
        let locked = app.selected_objs().iter().any(|id| app.drawables.iter().any(|d| d.id == *id && d.locked));
        v.push(if locked { item("Unlock", "Unlock") } else { item("Lock", "Lock") });
        v.push(sep);
        v.push(item("Copy Style", "Copy Style"));
        v.push(item("Paste Style", "Paste Style"));
        return Value::Array(v);
    }
    if let Some(sel) = app.sel {
        v.extend([item("Cut", "Cut"), item("Copy", "Copy"), item("Paste", "Paste"), item("Delete", "Delete")]);
        if sel.anchor == sel.cursor {
            v.push(json!({ "label": "Edit Cell", "action": "tbl:edit:CUR", "enabled": true }));
            v.push(item("Comment", "Comment"));
        }
        v.push(sep.clone());
        v.extend([item("Add Row Above", "Table > Add Row Above"), item("Add Row Below", "Table > Add Row Below"), item("Add Column Before", "Table > Add Column Before"), item("Add Column After", "Table > Add Column After")]);
        v.push(sep.clone());
        v.extend([item("Delete Row", "Table > Delete Row"), item("Delete Column", "Table > Delete Column")]);
        v.push(sep.clone());
        // Header Rows / Header Columns / Footer Rows: 0–5 each (canvas.rs table_menu)
        for (name, kind) in [("Header Rows", HeaderKind::Rows), ("Header Columns", HeaderKind::Columns), ("Footer Rows", HeaderKind::Footer)] {
            let cur = app.doc().map(|d| d.table_header_count(sel.table, kind)).unwrap_or(0);
            let kids: Vec<Value> = (0..=5u32).map(|n| json!({ "label": n.to_string(), "action": format!("cmd:{name} > {n}"), "enabled": true, "checked": cur == n })).collect();
            v.push(json!({ "label": name, "kids": kids }));
        }
        v.push(sep.clone());
        v.extend([item("Merge Cells", "Table > Merge Cells"), item("Unmerge Cells", "Table > Unmerge Cells")]);
        v.push(sep.clone());
        v.extend([item("Sort Ascending", "Sort Ascending"), item("Sort Descending", "Sort Descending")]);
        v.push(sep.clone());
        v.extend([item("Autofill Down", "Autofill Cells > Autofill"), item("Clear All", "Clear All")]);
        v.push(sep.clone());
        v.extend([item("Copy Style", "Copy Style"), item("Paste Style", "Paste Style")]);
        v.push(sep);
        v.push(json!({ "label": "Delete Table", "action": "tbl:deleteTable", "enabled": true }));
        return Value::Array(v);
    }
    v.push(item("Paste", "Paste"));
    v.push(item("Select All", "Select All"));
    Value::Array(v)
}

/// The menu bar as data: [{label, kids: [{label, command, enabled, checked, shortcut} |
/// {separator} | {label, kids}]}], as the desktop shows it now.
pub fn menus(app: &App) -> Value {
    use crate::menus::Item;
    fn item(app: &App, parent: &str, it: &Item) -> Value {
        if it.separator {
            return json!({ "separator": true });
        }
        if it.label == "Open Recent" {
            let kids: Vec<Value> = app.recent().iter().map(|p| json!({ "label": p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), "command": format!("open:{}", p.display()), "enabled": true })).collect();
            return json!({ "label": "Open Recent", "kids": kids, "enabled": !kids.is_empty() });
        }
        if !it.children.is_empty() {
            let kids: Vec<Value> = it.children.iter().map(|c| item(app, &it.label, c)).collect();
            return json!({ "label": it.label, "kids": kids, "enabled": true });
        }
        let name = crate::menus::command_name(parent, &it.label);
        let shortcut = match name.as_str() {
            "Font > Bigger" | "Font > Smaller" => None,
            "Zoom > Zoom In" => Some("Ctrl++".to_string()),
            "Zoom > Zoom Out" => Some("Ctrl+-".to_string()),
            _ => it.shortcut.clone(),
        };
        json!({ "label": crate::menus::shown_label(app, &name, &it.label), "command": name, "enabled": crate::menus::enabled(app, &name), "checked": crate::menus::checked(app, &name) == Some(true), "shortcut": shortcut })
    }
    Value::Array(crate::menus::menus().iter().map(|m| item(app, "", m)).collect())
}

// ------------------------------------------------------------------ the toolbar

/// A toolbar item without a popover (Text, Comment, Format, Organize, the commands).
pub fn toolbar_press(app: &mut App, ctx: &egui::Context, id: &str) {
    match id {
        "Text" => app.insert_text_box(),
        "Comment" => app.add_comment(),
        "Format" => app.toggle_inspector(InspTab::Table),
        "Organize" => app.toggle_inspector(InspTab::Organize),
        _ => {
            if let Some(c) = crate::toolbar::def(id).and_then(|d| d.command) {
                crate::menus::run(app, ctx, c);
            }
        }
    }
}

/// A toolbar popover's contents: View, Insert, Media as menu items; Table its styles; Shape the
/// shapes' outlines; Chart the document's chart styles.
pub fn popover(app: &App, id: &str) -> Value {
    let item = |label: &str, cmd: &str| json!({ "label": label, "action": format!("cmd:{cmd}"), "enabled": crate::menus::enabled(app, cmd) });
    let on = |label: &str, action: &str| json!({ "label": label, "action": action, "enabled": true });
    let sep = json!({ "separator": true });
    match id {
        "View" => json!([
            on(if app.inspector { "Hide Inspector" } else { "Show Inspector" }, "cmd:Inspector > Hide Inspector"),
            on("Show Find & Replace", "cmd:Find > Find…"),
            sep,
            on(if app.show_comments { "Hide Comments" } else { "Show Comments" }, "cmd:Comments > Hide Comments"),
            on(if app.show_guides { "Hide Guides" } else { "Show Guides" }, "cmd:Guides > Hide Guides"),
        ]),
        "Insert" => {
            let tables: Vec<Value> = nm_core::sheets::TABLE_STYLES.iter().enumerate().map(|(i, n)| on(n, &format!("table:{i}"))).collect();
            let charts: Vec<Value> = nm_core::charts::CHART_TYPES.iter().enumerate().map(|(i, (n, _))| on(n, &format!("chart:{i}"))).collect();
            let shapes: Vec<Value> = nm_core::sheets::SHAPES.iter().enumerate().map(|(i, n)| on(n, &format!("shape:{i}"))).collect();
            json!([
                { "label": "Table", "kids": tables },
                { "label": "Chart", "kids": charts },
                on("Text Box", "cmd:Text"),
                { "label": "Shape", "kids": shapes },
                on("Choose…", "media:Choose…"),
                sep,
                item("Comment", "Comment"),
                sep,
                on("Sheet", "cmd:Sheet"),
                sep,
                { "label": "Formula", "kids": [item("Sum", "Formula > Sum"), item("Average", "Formula > Average"), item("Minimum", "Formula > Minimum"), item("Maximum", "Formula > Maximum"), item("Count", "Formula > Count"), item("Product", "Formula > Product"), item("New Formula", "Formula > Edit Formula"), item("Stock Quote", "Formula > Stock Quote")] },
                item("Date & Time", "Date & Time"),
            ])
        }
        // Numbers' Media popover (toolbar.rs)
        "Media" => json!([
            on("Choose…", "media:Choose…"),
            item("Content Hub", "Browse Content Hub"),
            item("Record Audio", "Record Audio"),
            item("Web Video…", "Web Video…"),
            on("Image Gallery", "media:Image Gallery"),
            item("Photos and Videos", "Show Photo and Video Browser"),
            item("Music", "Show Music Browser"),
        ]),
        "Table" => {
            let styles: Vec<Value> = nm_core::sheets::TABLE_STYLES.iter().enumerate().map(|(i, n)| json!({ "name": n, "action": format!("table:{i}") })).collect();
            json!({ "styles": styles })
        }
        "Shape" => {
            use nm_core::kn_model::insert::BasicShape;
            let path_of = |b: BasicShape| -> Vec<Value> {
                nm_core::kn_model::path::from_path_source(&b.path_source(), 100.0, 100.0)
                    .els
                    .iter()
                    .map(|e| match e {
                        nm_core::kn_model::PathEl::Move(p) => json!(["M", p.x, p.y]),
                        nm_core::kn_model::PathEl::Line(p) => json!(["L", p.x, p.y]),
                        nm_core::kn_model::PathEl::Quad(c, p) => json!(["Q", c.x, c.y, p.x, p.y]),
                        nm_core::kn_model::PathEl::Cubic(a, b, p) => json!(["C", a.x, a.y, b.x, b.y, p.x, p.y]),
                        nm_core::kn_model::PathEl::Close => json!(["Z"]),
                    })
                    .collect()
            };
            let shapes: Vec<Value> = crate::chrome::SHAPE_TILES
                .iter()
                .filter(|n| !matches!(**n, "Text" | "Line"))
                .map(|name| {
                    let path: Vec<Value> = match *name {
                        "Text" => Vec::new(),
                        "Line" => vec![json!(["M", 8, 92]), json!(["L", 92, 8])],
                        "Rounded Rectangle" => path_of(BasicShape::RoundedRectangle),
                        "Circle" => path_of(BasicShape::Oval),
                        "Square" => path_of(BasicShape::Rectangle),
                        "Arrow" => path_of(BasicShape::Arrow),
                        "Triangle" => path_of(BasicShape::Triangle),
                        "Pentagon" => path_of(BasicShape::Pentagon),
                        "Diamond" => path_of(BasicShape::Diamond),
                        other => match nm_core::sheets::extra_shape(other) {
                            Some((els, (w, h))) => {
                                let k = 100.0 / w.max(h).max(1.0);
                                let (ox, oy) = ((100.0 - w * k) / 2.0, (100.0 - h * k) / 2.0);
                                let q = |x: f32, y: f32| (ox + x * k, oy + y * k);
                                els.iter()
                                    .map(|e| match *e {
                                        nm_core::sheets::PathEl::Move(x, y) => { let (x, y) = q(x, y); json!(["M", x, y]) }
                                        nm_core::sheets::PathEl::Line(x, y) => { let (x, y) = q(x, y); json!(["L", x, y]) }
                                        nm_core::sheets::PathEl::Curve(a, b, c) => { let (a, b, c) = (q(a.0, a.1), q(b.0, b.1), q(c.0, c.1)); json!(["C", a.0, a.1, b.0, b.1, c.0, c.1]) }
                                        nm_core::sheets::PathEl::Close => json!(["Z"]),
                                    })
                                    .collect()
                            }
                            None => Vec::new(),
                        },
                    };
                    json!({ "action": format!("tile:{name}"), "name": name, "path": path, "line": matches!(*name, "Line" | "Curve") })
                })
                .collect();
            json!({ "shapes": shapes })
        }
        "Chart" => {
            let Some(doc) = app.doc() else { return Value::Null };
            let styles: Vec<Value> = doc.chart_presets().iter().map(|p| json!({ "id": p, "colors": doc.chart_preset_colors(*p).iter().map(|c| json!([c.r, c.g, c.b, c.a])).collect::<Vec<_>>() })).collect();
            json!({ "styles": styles })
        }
        _ => Value::Null,
    }
}

/// A menu or popover item's action: "cmd:<menu command>", "table:i", "chart:i[:type]",
/// "chartstyle:<preset>:<name>", "shape:i", "tile:<name>", "sheet:…", "tbl:…".
pub fn action(app: &mut App, ctx: &egui::Context, a: &str) {
    let (kind, rest) = a.split_once(':').unwrap_or((a, ""));
    match kind {
        "cmd" => crate::menus::run(app, ctx, rest),
        "table" => {
            commit(app);
            app.insert_table(rest.parse().unwrap_or(0));
        }
        // a chart: by the Insert menu's index, or from the Chart popover "chart:<menu name>:<style preset id>"
        "chart" => {
            commit(app);
            match rest.parse::<usize>() {
                Ok(i) => app.insert_chart(i, None),
                Err(_) => {
                    let (name, preset) = rest.rsplit_once(':').unwrap_or((rest, ""));
                    crate::menus::run(app, ctx, &format!("Chart > {name}"));
                    if let (Ok(preset), Some(chart)) = (preset.parse::<ObjId>(), app.canvas.obj) {
                        if app.doc().and_then(|d| d.chart_preset(chart)) != Some(preset) {
                            app.merge_next_edit = true;
                            app.edit("Insert Chart", |d, h| d.apply_chart_preset(h, chart, preset));
                            app.merge_next_edit = false;
                        }
                    }
                }
            }
        }
        // the Chart popover: a type in a style ("chartstyle:<preset id>:<type name>")
        "chartstyle" => {
            let (preset, name) = rest.split_once(':').unwrap_or(("", rest));
            commit(app);
            crate::menus::run(app, ctx, &format!("Chart > {name}"));
            if let (Ok(preset), Some(chart)) = (preset.parse::<ObjId>(), app.canvas.obj) {
                if app.doc().and_then(|d| d.chart_preset(chart)) != Some(preset) {
                    app.merge_next_edit = true;
                    app.edit("Insert Chart", |d, h| d.apply_chart_preset(h, chart, preset));
                    app.merge_next_edit = false;
                }
            }
        }
        // the Shape popover's own tiles (a text box, a line, a line with an arrowhead), the
        // Insert menu's shapes by index
        "shape" => {
            commit(app);
            match rest {
                "text" => app.insert_text_box(),
                "line" => app.insert_shape(0),
                "arrow" => crate::menus::run(app, ctx, "Line > Line with Arrowhead"),
                _ => app.insert_shape(rest.parse().unwrap_or(0)),
            }
        }
        "media" => match rest {
            "Content Hub Shapes" => app.hub.show_tab(kn_ui::contenthub::Tab::Shapes),
            "Content Hub" => app.hub.show_hub(),
            "Image Gallery" => crate::menus::run(app, ctx, "Image Gallery"),
            _ => {}
        },
        "tile" => {
            commit(app);
            if let Some(name) = crate::chrome::SHAPE_TILES.iter().find(|n| **n == rest) {
                crate::chrome::insert_tile(app, name);
            }
        }
        "ctl" => control_action(app, ctx, rest),
        "insert" => crate::media::insert_path(app, std::path::PathBuf::from(rest)),
        // Insert > Image Gallery with the pictures the front end picked (one path a line)
        "gallery" => crate::media::gallery_from(app, &rest.lines().map(std::path::PathBuf::from).collect::<Vec<_>>()),
        "sheet" => sheet_action(app, rest),
        "tbl" => {
            // (the edit menu's Edit Cell: the cursor cell)
            if rest == "edit:CUR" {
                commit(app);
                canvas::start_edit(app, None);
            } else {
                table_action(app, rest);
            }
        }
        "title" => {
            // a table's title renamed in place: "title:<id>:<name>"
            let (id, name) = rest.split_once(':').unwrap_or(("", ""));
            if let Ok(id) = id.parse::<ObjId>() {
                let name = name.trim().to_string();
                let old = app.table(id).map(|(_, t)| t.name.clone()).unwrap_or_default();
                if !name.is_empty() && name != old {
                    app.edit("Table Name", |d, h| nm_core::sheets::rename_table(d, h, id, &name));
                }
            }
        }
        "msg" => app.message = None,
        // File > Close on a phone or tablet (the front end asked about unsaved changes): a new
        // spreadsheet, the chooser over it
        "close" => {
            commit(app);
            app.new_document();
            app.templates.chooser = true;
        }
        "find" => find_action(app, rest),
        "insp" => {
            app.insp_tab = match rest {
                "Cell" => InspTab::Cell,
                "Text" => InspTab::Text,
                "Arrange" => InspTab::Arrange,
                "Organize" => InspTab::Organize,
                _ => InspTab::Table,
            };
        }
        _ => {}
    }
}

// ------------------------------------------------------------------ sheets

/// The sheet tabs: [{name, hidden}] in order, and which shows.
pub fn sheets(app: &App) -> Value {
    let list: Vec<Value> = app
        .sheets()
        .iter()
        .map(|s| json!({ "name": s.name, "hidden": s.hidden, "color": app.wb.as_ref().and_then(|w| nm_core::sheets::tab_color(w.file(), s.id)).map(col) }))
        .collect();
    json!({ "sheets": list, "current": app.sheet })
}

/// "go:i", "add", "rename:i:<name>", "duplicate:i", "delete:i", "hide:i", "unhide:i", "move:i:to".
fn sheet_action(app: &mut App, a: &str) {
    let mut p = a.splitn(3, ':');
    let kind = p.next().unwrap_or("");
    let i: usize = p.next().and_then(|v| v.parse().ok()).unwrap_or(app.sheet);
    let rest = p.next().unwrap_or("");
    commit(app);
    match kind {
        "go" => app.go_to_sheet(i),
        "add" => app.add_sheet(),
        "rename" => {
            let name = rest.trim().to_string();
            if let Some(id) = app.sheets().get(i).map(|s| s.id).filter(|_| !name.is_empty()) {
                app.edit("Rename Sheet", |d, h| nm_core::sheets::rename_sheet(d, h, id, &name));
            }
        }
        "duplicate" => app.duplicate_sheet(i),
        "delete" => {
            if app.sheets().len() > 1 {
                app.edit("Delete Sheet", |d, h| nm_core::sheets::delete_sheet(d, h, i));
                let n = app.sheets().len();
                app.sheet = usize::MAX;
                app.go_to_sheet(i.min(n.saturating_sub(1)));
            }
        }
        "hide" => app.hide_sheet(i),
        "unhide" => {
            if let Some(id) = app.sheets().get(i).map(|s| s.id) {
                app.edit("Unhide Sheet", |d, h| nm_core::sheets::set_sheet_hidden(d, h, id, false));
                app.go_to_sheet(i);
            }
        }
        "move" => {
            let to: usize = rest.parse().unwrap_or(i);
            if to != i {
                let cur = app.sheets().get(app.sheet).map(|s| s.id);
                app.edit("Move Sheet", |d, h| nm_core::sheets::move_sheet(d, h, i, to));
                if let Some(k) = cur.and_then(|c| app.sheets().iter().position(|s| s.id == c)) {
                    app.sheet = k;
                }
            }
        }
        _ => {}
    }
}

// ------------------------------------------------------------------ the cell bar

fn one_line(s: &str) -> String {
    s.replace(['\n', '\u{2028}'], " ")
}

/// The bar at the foot (app.rs cell_bar): the selected cell's formula or value (and comment),
/// or several cells' quick sums.
pub fn cell_bar(app: &mut App) -> Value {
    let Some(sel) = app.sel else { return Value::Null };
    let (r0, c0, r1, c1) = sel.block();
    if (r0, c0) != (r1, c1) {
        let nums: Vec<f64> = app
            .table(sel.table)
            .map(|(_, t)| {
                (r0..=r1)
                    .flat_map(|r| (c0..=c1).map(move |c| (r, c)))
                    .filter(|(r, _)| t.row_heights.get(*r as usize).is_none_or(|h| *h > 0.0))
                    .filter_map(|(r, c)| {
                        let s = t.cells.get((r * t.cols + c) as usize)?;
                        let clean: String = s.chars().filter(|ch| ch.is_ascii_digit() || matches!(ch, '.' | '-' | 'E' | 'e')).collect();
                        if clean.is_empty() || s.chars().any(|ch| ch.is_alphabetic() && ch != 'E' && ch != 'e') { None } else { clean.parse::<f64>().ok().map(|v| if s.trim_end().ends_with('%') { v / 100.0 } else { v }) }
                    })
                    .collect()
            })
            .unwrap_or_default();
        let cells = (r1 - r0 + 1) * (c1 - c0 + 1);
        if nums.is_empty() {
            return json!({ "count": cells });
        }
        let sum: f64 = nums.iter().sum();
        let fmt = |v: f64| nm_core::kn_model::table::fmt_num((v * 1e4).round() / 1e4);
        let sums: Vec<Value> = [
            ("SUM", sum),
            ("AVG", sum / nums.len() as f64),
            ("MIN", nums.iter().copied().fold(f64::MAX, f64::min)),
            ("MAX", nums.iter().copied().fold(f64::MIN, f64::max)),
            ("COUNT", nums.len() as f64),
        ]
        .iter()
        .map(|(n, v)| json!([n, fmt(*v)]))
        .collect();
        return json!({ "sums": sums, "count": cells });
    }
    let (vr, c) = sel.cursor;
    let value = app.table(sel.table).and_then(|(_, t)| t.cells.get((vr * t.cols + c) as usize).cloned()).unwrap_or_default();
    let Some(r) = app.mrow(sel.table, vr) else {
        return json!({ "kind": "Category", "text": one_line(value.trim_start_matches("▼ ")) });
    };
    let formula = app.formula_of(sel.table, r, c);
    let comment = app.comments.get(&sel.table).and_then(|m| m.get(&(r, c))).cloned().map(|(t, a)| if a.is_empty() { t } else { format!("{t} — {a}") });
    json!({
        "ref": format!("{}{}", canvas::col_name(c), r + 1),
        "formula": formula,
        "value": one_line(&value),
        "comment": comment,
    })
}

// ------------------------------------------------------------------ keys

/// A key (egui's names), with Shift / Ctrl / Alt, as on the desktop canvas (canvas.rs
/// shortcuts): menu shortcuts with Ctrl, the selected cells' navigation, the objects' nudging,
/// the text box being edited.
pub fn key(app: &mut App, ctx: &egui::Context, name: &str, shift: bool, ctrl: bool, alt: bool) {
    use egui::Key;
    let Some(key) = Key::from_name(name) else { return };
    let moving = matches!(key, Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown | Key::Home | Key::End | Key::Backspace | Key::Delete);
    if ctrl && !(moving && app.canvas.editing.is_some()) && !(moving && app.sel.is_some()) {
        let mods = egui::Modifiers { shift, ctrl, alt, command: ctrl, ..Default::default() };
        if let Some(cmd) = crate::menus::shortcut_command(mods, key) {
            crate::menus::run(app, ctx, &cmd);
        }
        return;
    }
    if app.canvas.editing.is_some() {
        let tl = app.canvas.editing.as_ref().map(|e| e.id).and_then(|id| crate::textedit::layout_of(app, id)).map(|x| x.0);
        let mods = egui::Modifiers { shift, ctrl, alt, command: ctrl, ..Default::default() };
        if key == Key::Escape {
            crate::textedit::commit(app);
            return;
        }
        crate::textedit::handle_events(app, ctx, tl.as_ref(), vec![egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: mods }]);
        return;
    }
    if let Some(id) = app.canvas.obj {
        match key {
            Key::Delete | Key::Backspace => app.delete_objects(),
            Key::Escape => app.canvas.obj = None,
            Key::Tab => {
                let ids: Vec<ObjId> = app.drawables.iter().map(|d| d.id).collect();
                if let Some(i) = ids.iter().position(|o| *o == id) {
                    let n = ids.len();
                    app.canvas.obj = Some(ids[if shift { (i + n - 1) % n } else { (i + 1) % n }]);
                    app.sel = None;
                }
            }
            Key::Enter => {
                crate::textedit::begin(app, id, None);
            }
            _ => {
                let step = if shift { 10.0 } else { 1.0 };
                let d = match key {
                    Key::ArrowLeft => (-step, 0.0),
                    Key::ArrowRight => (step, 0.0),
                    Key::ArrowUp => (0.0, -step),
                    Key::ArrowDown => (0.0, step),
                    _ => return,
                };
                let all: Vec<ObjId> = app.drawables.iter().map(|d| d.id).collect();
                app.edit("Move", |doc, h| {
                    doc.move_by(h, id, d.0, d.1);
                    doc.update_connections_in(h, &all, &[id]);
                });
            }
        }
        return;
    }
    if app.sel.is_none() {
        return;
    }
    if alt && !ctrl {
        for (k, axis, after) in [(Key::ArrowDown, Axis::Row, true), (Key::ArrowUp, Axis::Row, false), (Key::ArrowRight, Axis::Column, true), (Key::ArrowLeft, Axis::Column, false)] {
            if key == k {
                canvas::insert_next_to(app, axis, after);
            }
        }
        return;
    }
    let far = 1 << 20;
    let step = |n: i32| if ctrl { n * far } else { n };
    match key {
        Key::ArrowDown => canvas::move_cursor(app, step(1), 0, shift),
        Key::ArrowUp => canvas::move_cursor(app, step(-1), 0, shift),
        Key::ArrowRight => canvas::move_cursor(app, 0, step(1), shift),
        Key::ArrowLeft => canvas::move_cursor(app, 0, step(-1), shift),
        Key::Tab => canvas::tab_move(app, shift),
        Key::Enter => canvas::return_move(app),
        Key::F2 => canvas::start_edit(app, None),
        Key::Delete | Key::Backspace => canvas::clear_cells(app),
        Key::Escape => {
            app.sel = None;
        }
        _ => {}
    }
    if matches!(key, Key::ArrowDown | Key::ArrowUp | Key::ArrowLeft | Key::ArrowRight) {
        app.canvas.return_col = None;
        reveal_cursor(app);
    }
}

/// The cursor cell kept in view.
fn reveal_cursor(app: &mut App) {
    let Some(sel) = app.sel else { return };
    let Some((_, g)) = grids(app).into_iter().find(|g| g.0 == sel.table) else { return };
    let (r, c) = sel.cursor;
    if (r as usize) + 1 < g.ys.len() && (c as usize) + 1 < g.xs.len() {
        app.canvas.reveal = Some(g.block(r, c, r, c));
    }
}

/// Typed text: into the text box being edited, over the selected cell (its editor opens with
/// it), or into a selected text box or shape.
pub fn text(app: &mut App, ctx: &egui::Context, t: &str) {
    if t.is_empty() {
        return;
    }
    if app.canvas.editing.is_some() {
        let tl = app.canvas.editing.as_ref().map(|e| e.id).and_then(|id| crate::textedit::layout_of(app, id)).map(|x| x.0);
        crate::textedit::handle_events(app, ctx, tl.as_ref(), vec![egui::Event::Text(t.to_string())]);
        return;
    }
    if let Some(e) = app.canvas.edit.as_mut() {
        e.text.push_str(t);
        return;
    }
    if let Some(id) = app.canvas.obj {
        if crate::textedit::begin(app, id, None) {
            crate::textedit::insert_at_caret(app, t);
        }
        return;
    }
    if let Some(sel) = app.sel {
        app.sel = Some(Sel::cell(sel.table, sel.cursor.0, sel.cursor.1));
        canvas::start_edit(app, Some(t.to_string()));
    }
}

/// Pasted text (the system clipboard), as Edit > Paste: into the text being edited, Values'
/// own copied cells whole, other text as values at the cursor, a new table on an empty sheet.
pub fn paste(app: &mut App, ctx: &egui::Context, t: &str) {
    if app.canvas.editing.is_some() {
        let tl = app.canvas.editing.as_ref().map(|e| e.id).and_then(|id| crate::textedit::layout_of(app, id)).map(|x| x.0);
        crate::textedit::handle_events(app, ctx, tl.as_ref(), vec![egui::Event::Paste(t.to_string())]);
        return;
    }
    if app.obj_clip_active && app.clip.is_some() {
        app.paste_here();
        return;
    }
    // (a table copied in another app is taken whole, as Values' own copy)
    let ours = crate::clip::is_ours(app, t) || crate::clip::adopt_html(app, t);
    if !ours && crate::clip::paste_pictures(app, Some(t)) {
        return;
    }
    if app.sel.is_none() {
        if ours {
            crate::clip::paste_new_table(app);
        }
        return;
    }
    if ours {
        crate::clip::paste(app, crate::clip::PasteMode::All);
    } else {
        canvas::paste(app, t);
    }
}

/// A sheet point's cell for the front end ({table, r, c} or null).
pub fn hit(app: &App, p: Pos2) -> Value {
    let s = to_sheet(app, p);
    match cell_at(app, s) {
        Some((t, r, c)) => json!({ "table": t, "r": r, "c": c }),
        None => Value::Null,
    }
}

#[allow(dead_code)]
fn unused(_: Vec2) {}

// ------------------------------------------------------------------ Organize (organize.rs)

thread_local! {
    /// The Organize inspector's tab: 0 Categories, 1 Sort, 2 Filter (Sort first, as Numbers).
    static ORG_TAB: std::cell::Cell<usize> = const { std::cell::Cell::new(1) };
}

/// Every filter rule in the chooser's order: (group, rule, label).
fn filter_rules() -> Vec<(&'static str, nm_core::organize::Rule, &'static str)> {
    nm_core::organize::Rule::GROUPS.iter().flat_map(|(g, rs)| rs.iter().map(move |(r, l)| (*g, *r, *l))).collect()
}

/// The Organize inspector (Numbers 15.4: Categories | Sort | Filter) as data.
pub fn organize_state(app: &App, t: ObjId) -> Value {
    let Some(td) = app.wb.as_ref().and_then(|w| w.table_data(t)) else { return Value::Null };
    let columns: Vec<String> = (0..td.cols).map(|c| crate::organize::col_label(&td, c)).collect();
    let categorized = app.categorized(t);
    let (rules, selected) = app.sorts.get(&t).cloned().unwrap_or_default();
    let all = filter_rules();
    let fl = app.filters.get(&t).cloned().unwrap_or_default();
    let filters: Vec<Value> = fl
        .rules
        .iter()
        .map(|r| {
            let quick: Vec<Value> = if r.rule == nm_core::organize::Rule::Quick {
                let mut counts: Vec<(String, usize)> = Vec::new();
                for row in td.header_rows..td.rows.saturating_sub(td.footer_rows) {
                    let v = td.cells.get((row * td.cols + r.col) as usize).map(|s| s.trim().to_string()).unwrap_or_default();
                    match counts.iter_mut().find(|c| c.0 == v) {
                        Some(c) => c.1 += 1,
                        None => counts.push((v, 1)),
                    }
                }
                counts.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
                counts.iter().map(|(v, n)| json!([v, n, r.values.contains(v)])).collect()
            } else {
                Vec::new()
            };
            json!({ "col": r.col, "rule": all.iter().position(|x| x.1 == r.rule).unwrap_or(0), "a": r.a, "b": r.b, "operands": r.rule.operands(), "quick": quick })
        })
        .collect();
    let mut groups_out: Vec<Value> = Vec::new();
    let c = app.cats.get(&t).cloned().unwrap_or_default();
    let from_file = c.levels.is_empty() && app.wb.as_ref().is_some_and(|w| nm_core::categories::groups(w.file(), t).is_some());
    if !c.levels.is_empty() {
        let mut probe = td.clone();
        let (_, groups) = crate::organize::categorize(&mut probe, &crate::organize::Cats { collapsed: Default::default(), ..c.clone() });
        let mut keys: Vec<(usize, crate::organize::GroupRow)> = groups.into_iter().collect();
        keys.sort_by_key(|k| k.0);
        groups_out = keys.iter().map(|(_, g)| json!({ "key": g.key, "name": g.key.rsplit(" › ").next().unwrap_or(&g.key), "level": g.level, "count": g.rows.len(), "closed": c.collapsed.contains(&g.key) })).collect();
    }
    let file_groups: Vec<Value> = if from_file { app.wb.as_ref().and_then(|w| nm_core::categories::groups(w.file(), t)).unwrap_or_default().iter().take(60).map(|g| json!([g.label, g.rows.len()])).collect() } else { Vec::new() };
    json!({
        "tab": ORG_TAB.with(|x| x.get()),
        "pivot": app.pivots.contains_key(&t),
        "columns": columns,
        "categorized": categorized,
        "sort": { "rules": rules.iter().map(|(c, up)| json!([c, up])).collect::<Vec<_>>(), "selected": selected },
        "filter": { "on": fl.on, "any": fl.any, "rules": filters },
        "ruleGroups": nm_core::organize::Rule::GROUPS.iter().map(|(g, rs)| json!([g, rs.iter().map(|(r, l)| json!([all.iter().position(|x| x.1 == *r).unwrap_or(0), l])).collect::<Vec<_>>()])).collect::<Vec<_>>(),
        "cats": { "on": c.on || from_file, "levels": c.levels.iter().map(|(c, d)| json!([c, d])).collect::<Vec<_>>(), "hideCol": c.hide_col, "groups": groups_out, "fromFile": from_file, "fileGroups": file_groups },
    })
}

/// An Organize control: "tab", "sortNow", "sortAsc" / "sortDesc" (the cursor's column),
/// "sort.add|remove|col|dir|scope", "filter.on|any|add|remove|rule|a|b|quick",
/// "cats.on|add|remove|up|down|col|dir|hide|toggle|collapseAll|expandAll". `v` the value,
/// or [index, value] for a rule's or level's own.
pub fn organize_set(app: &mut App, k: &str, v: &Value) {
    let ctx = egui::Context::default();
    let Some(t) = app.sel.map(|s| s.table) else { return };
    let Some(td) = app.wb.as_ref().and_then(|w| w.table_data(t)) else { return };
    let int = |v: &Value| v.as_u64().unwrap_or(0) as u32;
    let pair = |v: &Value| (v.get(0).and_then(Value::as_u64).unwrap_or(0) as usize, v.get(1).cloned().unwrap_or(Value::Null));
    let (part, key) = k.split_once('.').unwrap_or((k, ""));
    match part {
        "tab" => ORG_TAB.with(|x| x.set(v.as_u64().unwrap_or(1).min(2) as usize)),
        "sortAsc" => crate::menus::run(app, &ctx, "Sort Ascending"),
        "sortDesc" => crate::menus::run(app, &ctx, "Sort Descending"),
        "sortNow" => {
            let (rules, selected) = app.sorts.get(&t).cloned().unwrap_or_default();
            if rules.is_empty() || app.categorized(t) {
                return;
            }
            commit(app);
            let range = if selected {
                app.sel.map(|s| {
                    let b = s.block();
                    (app.mrow(t, b.0).unwrap_or(b.0), app.mrow(t, b.2).unwrap_or(b.2))
                })
            } else {
                None
            };
            if let Some(Err(e)) = app.edit("Sort", |d, h| d.sort_table_rows_by(h, t, &rules, range)) {
                app.notify(format!("Sort: {e}"));
            }
        }
        "sort" => {
            let (mut rules, mut selected) = app.sorts.get(&t).cloned().unwrap_or_default();
            match key {
                "add" => rules.push((int(v).min(td.cols.saturating_sub(1)), true)),
                "remove" => {
                    let i = int(v) as usize;
                    if i < rules.len() {
                        rules.remove(i);
                    }
                }
                "col" | "dir" => {
                    let (i, x) = pair(v);
                    if let Some(r) = rules.get_mut(i) {
                        if key == "col" {
                            r.0 = x.as_u64().unwrap_or(0) as u32;
                        } else {
                            r.1 = x.as_bool().unwrap_or(true);
                        }
                    }
                }
                "scope" => selected = v.as_bool().unwrap_or(false),
                _ => {}
            }
            app.sorts.insert(t, (rules, selected));
        }
        "filter" => {
            let before = app.filters.get(&t).cloned().unwrap_or_default();
            let mut fl = before.clone();
            let all = filter_rules();
            let mut typing = false;
            match key {
                "on" => fl.on = v.as_bool().unwrap_or(false),
                "any" => fl.any = v.as_bool().unwrap_or(false),
                "add" => {
                    let c = int(v);
                    let vals = app.wb.as_ref().map(|w| w.doc.cell_values(t)).unwrap_or_default();
                    let numeric = (td.header_rows..td.rows).any(|r| matches!(vals.get(&(r, c)), Some(nm_core::kn_model::table::CellValue::Number(_))));
                    fl.rules.push(nm_core::organize::FilterRule::new(c, if numeric { nm_core::organize::Rule::Greater } else { nm_core::organize::Rule::Contains }));
                    fl.on = true;
                }
                "remove" => {
                    let i = int(v) as usize;
                    if i < fl.rules.len() {
                        fl.rules.remove(i);
                    }
                }
                _ => {
                    let (i, x) = pair(v);
                    if let Some(r) = fl.rules.get_mut(i) {
                        match key {
                            "rule" => {
                                if let Some(rule) = all.get(x.as_u64().unwrap_or(0) as usize).map(|a| a.1) {
                                    r.rule = rule;
                                }
                            }
                            "a" => {
                                r.a = x.as_str().unwrap_or_default().to_string();
                                typing = true;
                            }
                            "b" => {
                                r.b = x.as_str().unwrap_or_default().to_string();
                                typing = true;
                            }
                            "quick" => r.values = x.as_array().map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect()).unwrap_or_default(),
                            _ => {}
                        }
                    }
                }
            }
            if fl != before {
                app.filters.insert(t, fl);
                commit(app);
                app.merge_next_edit = typing;
                let _ = app.edit("Filter", |_, _| ());
                app.merge_next_edit = false;
            }
        }
        "cats" => {
            let before = app.cats.get(&t).cloned().unwrap_or_default();
            let mut c = before.clone();
            match key {
                "on" => c.on = v.as_bool().unwrap_or(false),
                "add" => {
                    if c.levels.len() < 5 {
                        c.levels.push((int(v), false));
                        c.on = true;
                    }
                }
                "remove" | "up" | "down" => {
                    let i = int(v) as usize;
                    if i < c.levels.len() {
                        match key {
                            "remove" => {
                                c.levels.remove(i);
                            }
                            "up" if i > 0 => c.levels.swap(i, i - 1),
                            "down" if i + 1 < c.levels.len() => c.levels.swap(i, i + 1),
                            _ => {}
                        }
                        c.collapsed.clear();
                    }
                }
                "col" | "dir" => {
                    let (i, x) = pair(v);
                    if let Some(l) = c.levels.get_mut(i) {
                        if key == "col" {
                            l.0 = x.as_u64().unwrap_or(0) as u32;
                        } else {
                            l.1 = x.as_bool().unwrap_or(false);
                        }
                    }
                }
                "hide" => c.hide_col = v.as_bool().unwrap_or(false),
                "toggle" => {
                    let k = v.as_str().unwrap_or_default().to_string();
                    if !c.collapsed.remove(&k) {
                        c.collapsed.insert(k);
                    }
                }
                "collapseAll" => {
                    let mut probe = td.clone();
                    let (_, groups) = crate::organize::categorize(&mut probe, &crate::organize::Cats { collapsed: Default::default(), ..c.clone() });
                    c.collapsed = groups.into_values().map(|g| g.key).collect();
                }
                "expandAll" => c.collapsed.clear(),
                _ => {}
            }
            if c != before {
                commit(app);
                app.cats.insert(t, c);
                app.version += 1;
            }
        }
        _ => {}
    }
}

// ------------------------------------------------------------------ Find & Replace

/// The find bar: {query, count, cur, replace} or null when it's closed.
pub fn find_state(app: &App) -> Value {
    let Some((q, hits, cur)) = app.find.as_ref() else { return Value::Null };
    json!({ "query": q, "count": hits.len(), "cur": cur, "case": app.find_case, "words": app.find_words, "replace": app.replace })
}

/// "q:<text>", "step:<1 next|0>", "replaceOpen", "replaceClose", "with:<text>", "replace",
/// "replaceAll", "close".
fn find_action(app: &mut App, a: &str) {
    let (k, v) = a.split_once(':').unwrap_or((a, ""));
    match k {
        "q" => {
            let res = app.search(v);
            let any = !res.is_empty();
            app.find = Some((v.to_string(), res, 0));
            if any {
                app.show_match(0);
            }
        }
        "step" => app.find_step(v == "1"),
        // Search Options: Match Case, Whole Words (the matches found again, the first shown)
        "case" | "words" => {
            if k == "case" {
                app.find_case = v == "1";
            } else {
                app.find_words = v == "1";
            }
            let q = app.find.as_ref().map(|f| f.0.clone()).unwrap_or_default();
            let res = app.search(&q);
            let any = !res.is_empty();
            app.find = Some((q, res, 0));
            if any {
                app.show_match(0);
            }
        }
        "replaceOpen" => app.replace = Some(app.replace.clone().unwrap_or_default()),
        "replaceClose" => app.replace = None,
        "with" => app.replace = Some(v.to_string()),
        "replace" => app.replace_matches(false),
        "replaceAll" => app.replace_matches(true),
        "close" => {
            app.find = None;
            app.replace = None;
        }
        _ => {}
    }
}

/// Table `id`'s frozen header rows (`cols` false) or its header columns over rows r0..=r1,
/// drawn alone at `scale` pixels a point: the picture and its size in sheet points.
pub fn header(app: &mut App, id: ObjId, cols: bool, r0: u32, r1: u32, scale: f32) -> Option<(tiny_skia::Pixmap, (f32, f32))> {
    let d = app.drawables.iter().find(|d| d.id == id)?.clone();
    let wb = app.wb.as_ref()?;
    if cols { app.canvas.tiles.header_cols_pixmap(&wb.doc, &d, scale, r0, r1) } else { app.canvas.tiles.header_rows_pixmap(&wb.doc, &d, scale) }
}
