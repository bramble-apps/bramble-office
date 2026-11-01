//! The slide canvas as data, for front ends that draw it themselves (the Flutter app, ffi.rs):
//! taps and drags in slide points come in, the same selection, move, resize, rotate, line-end
//! and text-editing logic as the desktop canvas runs on them, and what to draw over the slide
//! (selection frames, handles, the caret and the text selection, a drag going on) goes out as
//! data. The front end pans and zooms the slide itself; `tol` is a finger's reach in slide
//! points at the zoom it shows.

use kn_format::ObjId;
use kn_model::{DrawableKind, Geometry, Point, SlideScene};
use kn_render::text::caret;
use serde_json::{Value, json};

use super::{Drag, finish_drag, handle_points, hit, line_ends, resized, select_box};
use crate::app::App;

fn pt(p: Point) -> Value {
    json!([p.x, p.y])
}

/// The scene being edited: the one rebuilt after the last keystroke while typing, else the
/// current slide's.
pub fn scene(app: &mut App) -> Option<SlideScene> {
    if app.canvas.editing.as_ref().is_some_and(|e| e.scene.is_none()) {
        let fresh = app.doc.as_ref().and_then(|d| d.scene(&app.slides, app.cur));
        if let Some(e) = app.canvas.editing.as_mut() {
            e.scene = fresh;
        }
    }
    if let Some(s) = app.canvas.editing.as_ref().and_then(|e| e.scene.clone()) {
        return Some(s);
    }
    app.scene(app.cur)
}

/// The scene with the drag going on applied (the moved, resized or rotated objects at their
/// live places), for drawing the selection alone while it moves.
pub fn live_scene(app: &mut App) -> Option<SlideScene> {
    let mut sc = scene(app)?;
    match app.canvas.drag.clone() {
        Some(Drag::Move { delta, .. }) => {
            for d in sc.drawables.iter_mut().filter(|d| app.sel.contains(&d.id)) {
                d.geom.x += delta.0;
                d.geom.y += delta.1;
            }
        }
        Some(Drag::Resize { id, cur, .. } | Drag::Rotate { id, cur, .. }) => {
            if let Some(d) = sc.drawables.iter_mut().find(|d| d.id == id) {
                let (sx, sy) = (cur.w / d.geom.w.max(0.01), cur.h / d.geom.h.max(0.01));
                if let DrawableKind::Shape(s) = &mut d.kind {
                    super::scale_path(&mut s.path, sx, sy);
                }
                d.geom = cur;
            }
        }
        Some(Drag::LineEnd { id, a, b, .. }) => {
            if let Some(d) = sc.drawables.iter_mut().find(|d| d.id == id) {
                if let DrawableKind::Shape(s) = &mut d.kind {
                    let (lo, hi) = (Point::new(a.x.min(b.x), a.y.min(b.y)), Point::new(a.x.max(b.x), a.y.max(b.y)));
                    d.geom = Geometry { x: lo.x, y: lo.y, w: (hi.x - lo.x).max(0.01), h: (hi.y - lo.y).max(0.01), angle: 0.0, ..d.geom };
                    s.path.els = vec![kn_model::PathEl::Move(Point::new(a.x - lo.x, a.y - lo.y)), kn_model::PathEl::Line(Point::new(b.x - lo.x, b.y - lo.y))];
                }
            }
        }
        _ => {}
    }
    Some(sc)
}

/// Objects the front end draws apart while they move (the slide is drawn without them).
pub fn moving(app: &App) -> Vec<ObjId> {
    match &app.canvas.drag {
        Some(Drag::Move { .. }) => app.sel.clone(),
        Some(Drag::Resize { id, .. } | Drag::Rotate { id, .. } | Drag::LineEnd { id, .. }) => vec![*id],
        _ => Vec::new(),
    }
}

/// What's under a point: "handle" (of the selected object: resize), "lineEnd", "editing" (the
/// text being edited), "selected", "object", or "none".
pub fn hit_test(app: &mut App, p: Point, tol: f32) -> Value {
    let Some(sc) = scene(app) else { return json!({ "what": "none" }) };
    if let Some(ed) = app.canvas.editing.as_ref().map(|e| e.id) {
        if let Some(d) = sc.drawables.iter().find(|d| d.id == ed) {
            if crate::textedit::contains(&d.geom, p) {
                return json!({ "what": "editing", "id": ed });
            }
        }
    }
    if let Some(h) = handle_at(app, &sc, p, tol) {
        return json!({ "what": if h.1 { "lineEnd" } else { "handle" }, "handle": h.0 });
    }
    match hit(app, &sc, p, tol * 0.5) {
        Some(id) if app.sel.contains(&id) => json!({ "what": "selected", "id": id }),
        Some(id) => json!({ "what": "object", "id": id }),
        None => json!({ "what": "none" }),
    }
}

/// The handle of the one selected object under `p`: (handle, a line's end?).
fn handle_at(app: &mut App, sc: &SlideScene, p: Point, tol: f32) -> Option<(usize, bool)> {
    let &[id] = &app.sel[..] else { return None };
    let d = sc.drawables.iter().find(|d| d.id == id)?;
    if d.locked {
        return None;
    }
    if let Some((a, b)) = line_ends(d) {
        return [a, b].iter().position(|q| (q.x - p.x).hypot(q.y - p.y) <= tol).map(|i| (i, true));
    }
    let g = select_box(app, d);
    let mut best = None;
    for (i, q) in handle_points(&g).iter().enumerate() {
        let s = g.to_slide(*q);
        let dist = (s.x - p.x).hypot(s.y - p.y);
        if dist <= tol && best.is_none_or(|(_, b)| dist < b) {
            best = Some((i, dist));
        }
    }
    best.map(|(i, _)| (i, false))
}

/// A tap: `count` 1, 2 (double) or 3. On an object it selects it (Shift adds or removes it); on
/// the selected text object, or a double tap on any, it starts editing its text at that place
/// (a double tap in the text selects the word, a triple the paragraph); elsewhere the selection
/// ends.
pub fn tap(app: &mut App, p: Point, tol: f32, count: u32, shift: bool) {
    let Some(sc) = scene(app) else { return };
    if app.pen.is_some() {
        return;
    }
    // point editing: a node picked, else it ends
    if app.point_edit.as_ref().is_some_and(|pe| app.sel == [pe.id]) {
        match point_hit(app, &sc, p, tol) {
            Some(h) => {
                if let Some(pe) = app.point_edit.as_mut() {
                    pe.selected = Some((h.0, h.1));
                }
            }
            None => app.point_edit = None,
        }
        return;
    }
    app.nav_focus = false;
    // in the text being edited: the caret there, or a word / the paragraph
    if let Some(ed_id) = app.canvas.editing.as_ref().map(|e| e.id) {
        if let Some(d) = sc.drawables.iter().find(|d| d.id == ed_id) {
            if crate::textedit::contains(&d.geom, p) {
                if let Some((tl, g)) = crate::textedit::stored_layout(app.renderer.text_layout(d), d) {
                    crate::textedit::point_to(app, &tl, &g, p, false);
                    if count >= 2 {
                        select_unit(app, count >= 3);
                    }
                }
                return;
            }
        }
        crate::textedit::commit(app);
    }
    app.canvas.cell = None;
    let Some(id) = hit(app, &sc, p, tol * 0.5) else {
        if !shift {
            app.sel.clear();
        }
        return;
    };
    let d = sc.drawables.iter().find(|d| d.id == id).cloned();
    let has_text = d.as_ref().is_some_and(|d| matches!(&d.kind, DrawableKind::Shape(s) if s.text.is_some()));
    if shift {
        match app.sel.iter().position(|s| *s == id) {
            Some(i) => {
                app.sel.remove(i);
            }
            None => app.sel.push(id),
        }
        return;
    }
    let was = app.sel == [id];
    // a table already selected: a tap picks a cell, a double tap edits it (canvas.rs)
    if d.as_ref().is_some_and(|d| matches!(d.kind, DrawableKind::Table(_))) && (was || count >= 2) {
        if let Some((r, c, text)) = super::table_cell_at(app, &sc, id, p) {
            app.sel = vec![id];
            app.canvas.table_cursor = Some((id, r, c));
            app.canvas.table_range = None;
            if count >= 2 {
                let text = app.doc.as_ref().and_then(|doc| doc.formula_text(id, r, c)).unwrap_or(text);
                app.canvas.cell = Some((id, r, c, text, true));
            }
            return;
        }
    }
    app.canvas.table_cursor = None;
    app.sel = vec![id];
    if has_text && !d.as_ref().is_some_and(|d| d.locked) && (was || count >= 2) {
        if crate::textedit::begin(app, &sc, id, Some(p)) && count >= 2 {
            select_unit(app, count >= 3);
        }
    }
}

/// The word (or the paragraph) around the caret selected.
fn select_unit(app: &mut App, para: bool) {
    let Some((storage, i)) = app.canvas.editing.as_ref().map(|e| (e.storage, e.cursor)) else { return };
    let text = app.doc.as_ref().map(|d| d.storage_text(storage)).unwrap_or_default();
    let i = i.min(text.len());
    let (a, b) = if para {
        (text[..i].rfind('\n').map(|k| k + 1).unwrap_or(0), text[i..].find('\n').map(|k| i + k).unwrap_or(text.len()))
    } else {
        let word = |c: char| c.is_alphanumeric() || c == '\'' || c == '’';
        let a = text[..i].char_indices().rev().take_while(|(_, c)| word(*c)).last().map(|(k, _)| k).unwrap_or(i);
        let b = text[i..].char_indices().find(|(_, c)| !word(*c)).map(|(k, _)| i + k).unwrap_or(text.len());
        (a, b)
    };
    if let Some(ed) = app.canvas.editing.as_mut() {
        ed.anchor = a;
        ed.cursor = b;
    }
}

/// A touch and hold: the object there selected (if it wasn't), for the edit menu.
pub fn hold(app: &mut App, p: Point, tol: f32) {
    let Some(sc) = scene(app) else { return };
    if let Some(ed_id) = app.canvas.editing.as_ref().map(|e| e.id) {
        if sc.drawables.iter().any(|d| d.id == ed_id && crate::textedit::contains(&d.geom, p)) {
            if let Some(d) = sc.drawables.iter().find(|d| d.id == ed_id) {
                if let Some((tl, g)) = crate::textedit::stored_layout(app.renderer.text_layout(d), d) {
                    crate::textedit::point_to(app, &tl, &g, p, false);
                    select_unit(app, false);
                }
            }
            return;
        }
        crate::textedit::commit(app);
    }
    match hit(app, &sc, p, tol * 0.5) {
        Some(id) if !app.sel.contains(&id) => app.sel = vec![id],
        Some(_) => {}
        None => app.sel.clear(),
    }
}

/// The caret, or one end of the text selection, moved to a slide point (`end` 0: the caret
/// alone, 1: the selection's moving end, 2: its other end), as the selection handles drag.
pub fn text_point(app: &mut App, p: Point, end: u32) {
    let Some(sc) = scene(app) else { return };
    let Some(id) = app.canvas.editing.as_ref().map(|e| e.id) else { return };
    let Some(d) = sc.drawables.iter().find(|d| d.id == id) else { return };
    let Some((tl, g)) = crate::textedit::stored_layout(app.renderer.text_layout(d), d) else { return };
    let mut l = g;
    l.hflip = false;
    l.vflip = false;
    let lp = l.to_local(p);
    let i = kn_render::text::hit_index(&tl, lp.x, lp.y);
    let Some(ed) = app.canvas.editing.as_mut() else { return };
    match end {
        0 => {
            ed.cursor = i;
            ed.anchor = i;
        }
        1 => ed.cursor = i,
        _ => ed.anchor = i,
    }
}

/// A drag: `kind` 1 starts (at what's there: a handle resizes, a line's end moves it, an object
/// moves the selection, selecting it first), 0 moves, 2 ends (the change made, one undo step),
/// 3 cancels. `ctrl` on a handle rotates (as the desktop's Ctrl-drag), `shift` keeps the aspect.
pub fn drag(app: &mut App, kind: u32, p: Point, tol: f32, shift: bool, ctrl: bool) {
    let Some(sc) = scene(app) else { return };
    // Draw With Pen: a stroke drawn becomes a freehand line (canvas.rs pen_mode)
    if app.pen.is_some() {
        pen_drag(app, &sc, kind, p);
        return;
    }
    // Make Editable: the nodes and their handles move (canvas.rs point_mode)
    if app.point_edit.as_ref().is_some_and(|pe| app.sel == [pe.id]) {
        point_drag(app, &sc, kind, p, tol);
        return;
    }
    match kind {
        1 => {
            if app.canvas.editing.is_some() {
                crate::textedit::commit(app);
            }
            app.canvas.drag = None;
            app.canvas.guides.clear();
            // a guide grabbed (away from any object) moves; off the slide it goes
            if app.show_guides && hit(app, &sc, p, tol * 0.5).is_none() {
                let gs = app.doc.as_ref().map(|d| d.guides(sc.slide)).unwrap_or_default();
                let near = |g: &kn_model::slides::Guide| if g.vertical { (p.x - g.pos).abs() } else { (p.y - g.pos).abs() } < tol * 0.4;
                if let Some(i) = gs.iter().position(near) {
                    app.canvas.drag = Some(Drag::Guide { index: Some(i), vertical: gs[i].vertical, pos: gs[i].pos });
                    return;
                }
            }
            if let Some((h, line)) = handle_at(app, &sc, p, tol) {
                let id = app.sel[0];
                let Some(d) = sc.drawables.iter().find(|d| d.id == id) else { return };
                if line {
                    if let Some((a, b)) = line_ends(d) {
                        app.canvas.drag = Some(Drag::LineEnd { id, end: h, a, b });
                    }
                    return;
                }
                let g = select_box(app, d);
                app.canvas.drag = Some(if ctrl {
                    let c = g.to_slide(Point::new(g.w / 2.0, g.h / 2.0));
                    Drag::Rotate { id, orig: g, start_angle: (p.y - c.y).atan2(p.x - c.x), cur: g }
                } else {
                    Drag::Resize { id, handle: h, orig: g, cur: g }
                });
                return;
            }
            match hit(app, &sc, p, tol * 0.5) {
                Some(id) => {
                    if !app.sel.contains(&id) {
                        app.sel = vec![id];
                    }
                    if app.sel.iter().any(|s| sc.drawables.iter().any(|d| d.id == *s && d.locked)) {
                        return;
                    }
                    app.canvas.drag = Some(Drag::Move { start: p, delta: (0.0, 0.0) });
                }
                None => {
                    if !shift {
                        app.sel.clear();
                    }
                    app.canvas.drag = Some(Drag::Rubber { start: egui::pos2(p.x, p.y), cur: egui::pos2(p.x, p.y) });
                }
            }
        }
        0 => match app.canvas.drag.as_mut() {
            Some(Drag::Guide { vertical, pos, .. }) => *pos = if *vertical { p.x } else { p.y },
            Some(Drag::Move { start, .. }) => {
                // alignment guides: each selected object snaps by itself, the smallest correction wins (canvas.rs)
                let d = (p.x - start.x, p.y - start.y);
                let opts = snap_opts(&app.settings, tol, ctrl);
                let sel = app.sel.clone();
                let others = super::other_boxes(&mut app.renderer, &sc, &sel);
                let boxes: Vec<crate::snap::Bx> = sc.drawables.iter().filter(|x| sel.contains(&x.id)).map(|x| { let (lo, hi) = app.renderer.effective_geometry(x).bounds(); crate::snap::Bx::new(lo, hi) }).collect();
                let mut best: Option<((f32, f32), Vec<crate::snap::Guide>, f32)> = None;
                for b in boxes {
                    let (d2, g) = crate::snap::snap_move(b, &others, super::slide_box(&sc), d, &opts);
                    let fix = (d2.0 - d.0).abs() + (d2.1 - d.1).abs();
                    if !g.is_empty() && best.as_ref().is_none_or(|x| fix < x.2) {
                        best = Some((d2, g, fix));
                    }
                }
                let (d2, g) = best.map(|b| (b.0, b.1)).unwrap_or((d, Vec::new()));
                if let Some(Drag::Move { delta, .. }) = app.canvas.drag.as_mut() {
                    *delta = d2;
                }
                app.canvas.guides = g;
            }
            Some(Drag::Resize { id, handle, orig, cur }) => {
                let keep = shift || sc.drawables.iter().any(|d| app.sel.contains(&d.id) && matches!(d.kind, DrawableKind::Image(_)));
                *cur = resized(orig, *handle, orig.to_local(p), keep);
                let (id, handle, mut g) = (*id, *handle, *cur);
                app.canvas.guides.clear();
                if g.angle.rem_euclid(360.0) == 0.0 && !keep {
                    let opts = snap_opts(&app.settings, tol, ctrl);
                    let others = super::other_boxes(&mut app.renderer, &sc, &[id]);
                    let b = crate::snap::Bx { x0: g.x, y0: g.y, x1: g.x + g.w, y1: g.y + g.h };
                    let (b, gd) = crate::snap::snap_resize(b, super::handle_edges(handle), &others, super::slide_box(&sc), &opts);
                    (g.x, g.y, g.w, g.h) = (b.x0, b.y0, b.x1 - b.x0, b.y1 - b.y0);
                    app.canvas.guides = gd;
                    if let Some(Drag::Resize { cur, .. }) = app.canvas.drag.as_mut() {
                        *cur = g;
                    }
                }
            }
            Some(Drag::Rotate { orig, start_angle, cur, .. }) => {
                let c = orig.to_slide(Point::new(orig.w / 2.0, orig.h / 2.0));
                let a = (p.y - c.y).atan2(p.x - c.x);
                let mut deg = orig.angle + (a - *start_angle).to_degrees();
                // (snaps to 15° steps within 3°, as with a Shift on the desktop)
                let snap = (deg / 15.0).round() * 15.0;
                if shift || (deg - snap).abs() < 3.0 {
                    deg = snap;
                }
                *cur = Geometry { angle: deg.rem_euclid(360.0), ..*orig };
            }
            Some(Drag::LineEnd { end, a, b, .. }) => {
                if *end == 0 {
                    *a = p;
                } else {
                    *b = p;
                }
            }
            Some(Drag::Rubber { cur, .. }) => *cur = egui::pos2(p.x, p.y),
            _ => {}
        },
        2 => {
            app.canvas.guides.clear();
            if let Some(d) = app.canvas.drag.take() {
                let view = super::View { rect: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(sc.size.w, sc.size.h)), scale: 1.0 };
                finish_drag(app, &sc, d, &view);
            }
        }
        _ => app.canvas.drag = None,
    }
}

/// A frame's corners on the slide, clockwise from its top-left.
fn corners(g: &Geometry) -> Value {
    json!([pt(g.to_slide(Point::new(0.0, 0.0))), pt(g.to_slide(Point::new(g.w, 0.0))), pt(g.to_slide(Point::new(g.w, g.h))), pt(g.to_slide(Point::new(0.0, g.h)))])
}

/// What's drawn over the slide (slide points): the selected objects' frames ({id, corners,
/// handles, line: [a, b], locked}), the text being edited ({frame, caret: [top, bottom], rects:
/// [quads], ends: [[top, bottom], [top, bottom]]}), the rubber band and the drag going on.
pub fn overlays(app: &mut App) -> Value {
    let Some(sc) = live_scene(app) else { return Value::Null };
    let dragging = app.canvas.drag.is_some();
    let editing_id = app.canvas.editing.as_ref().map(|e| e.id);
    let mut sel = Vec::new();
    for id in app.sel.clone() {
        let Some(d) = sc.drawables.iter().find(|d| d.id == id) else { continue };
        if Some(id) == editing_id || app.point_edit.as_ref().is_some_and(|pe| pe.id == id) {
            continue;
        }
        let g = select_box(app, d);
        let line = line_ends(d).map(|(a, b)| json!([pt(a), pt(b)]));
        sel.push(json!({ "id": id, "corners": corners(&g), "handles": app.sel.len() == 1 && !d.locked, "line": line, "locked": d.locked, "angle": g.angle }));
    }
    let mut editing = Value::Null;
    if let (Some(id), Some(ed)) = (editing_id, app.canvas.editing.as_ref().map(|e| (e.cursor, e.anchor))) {
        if let Some(d) = sc.drawables.iter().find(|d| d.id == id) {
            if let Some((tl, g)) = crate::textedit::stored_layout(app.renderer.text_layout(d), d) {
                let mut g2 = g;
                g2.hflip = false;
                g2.vflip = false;
                let map = |x: f32, y: f32| pt(g2.to_slide(Point::new(x, y)));
                let (a, b) = (ed.0.min(ed.1), ed.0.max(ed.1));
                let mut rects = Vec::new();
                if a < b {
                    for line in &tl.lines {
                        if line.end < a || line.start > b || (line.start == line.end && !(a..b).contains(&line.start)) {
                            continue;
                        }
                        let x0 = caret(&tl, a.max(line.start)).filter(|_| a >= line.start).map(|c| c.0).unwrap_or(line.x);
                        let x1 = if b <= line.end { caret(&tl, b).map(|c| c.0).unwrap_or(line.x + line.width) } else { line.x + line.width + 6.0 };
                        rects.push(json!([map(x0, line.top), map(x1, line.top), map(x1, line.top + line.height), map(x0, line.top + line.height)]));
                    }
                }
                let at = |i: usize| caret(&tl, i).map(|(x, top, h)| json!([map(x, top), map(x, top + h)]));
                editing = json!({
                    "id": id,
                    "frame": corners(&g),
                    "caret": if a == b { at(ed.0) } else { None },
                    "rects": rects,
                    "ends": if a < b { json!([at(a), at(b)]) } else { Value::Null },
                    "version": app.canvas.editing.as_ref().map(|e| e.version).unwrap_or(0),
                });
            }
        }
    }
    let rubber = match &app.canvas.drag {
        Some(Drag::Rubber { start, cur }) => json!([start.x.min(cur.x), start.y.min(cur.y), (start.x - cur.x).abs(), (start.y - cur.y).abs()]),
        _ => Value::Null,
    };
    // empty title and body placeholders: Keynote's grey prompt and outline (canvas.rs)
    let mut prompts = Vec::new();
    for d in &sc.drawables {
        if let (Some(kind), DrawableKind::Shape(s)) = (d.placeholder, &d.kind) {
            if s.text.as_ref().is_some_and(|t| t.text.is_empty()) && !d.from_template && editing_id != Some(d.id) {
                let text = match kind {
                    kn_model::PlaceholderKind::Title => "Double-tap to add title",
                    kn_model::PlaceholderKind::Body => "Double-tap to add text",
                    _ => continue,
                };
                let size = s.text.as_ref().and_then(|t| t.paragraphs.first()).and_then(|p| p.runs.first()).map(|r| r.style.size).unwrap_or(48.0);
                let g = d.geom;
                prompts.push(json!({ "rect": [g.x, g.y, g.w, g.h], "text": text, "size": size * 0.6 }));
            }
        }
    }
    // View > Comments: a sticky note at each commented object's top-left corner
    let comments: Vec<Value> = if app.show_comments {
        sc.drawables
            .iter()
            .filter(|d| !d.from_template)
            .filter_map(|d| {
                let c = app.doc.as_ref()?.comment(d.id)?;
                let (lo, _) = app.renderer.effective_geometry(d).bounds();
                Some(json!({ "id": d.id, "at": pt(lo), "color": c.color.map(|k| json!([k.r, k.g, k.b])), "author": c.author, "text": c.text }))
            })
            .collect()
    } else {
        Vec::new()
    };
    // a selected image gallery: arrows to flip through it (canvas.rs gallery controls)
    let gallery = app.sel.first().and_then(|id| sc.drawables.iter().find(|d| d.id == *id)).and_then(|d| match &d.kind {
        DrawableKind::Image(im) if im.gallery.len() > 1 && app.sel.len() == 1 => {
            let n = im.gallery.len();
            Some(json!({ "id": d.id, "corners": corners(&d.geom), "count": n, "shown": im.gallery_shown.min(n - 1) }))
        }
        _ => None,
    });
    json!({
        "gallery": gallery,
        "guides": guides_overlay(app, &sc),
        "pen": app.pen.as_ref().map(|pts| json!({ "stroke": pts.iter().map(|q| pt(*q)).collect::<Vec<_>>(), "color": app.pen_color.to_array(), "width": app.pen_width })),
        "points": points_overlay(app, &sc),
        "mask": app.mask_edit.and_then(|id| {
            let d = sc.drawables.iter().find(|d| d.id == id)?;
            let m = super::mask_box(d)?;
            Some(json!({ "id": id, "at": pt(Point::new(m.x + m.w / 2.0, m.y + m.h)), "size": (d.geom.w / m.w.max(0.01) * 100.0).clamp(100.0, 500.0) }))
        }),
        "prompts": prompts,
        "comments": comments,
        "table": table_overlay(app, &sc),
        "selection": sel,
        "editing": editing,
        "rubber": rubber,
        "dragging": dragging,
        "moving": moving(app),
        "size": [sc.size.w, sc.size.h],
    })
}

/// The cells of the selected table: the cell picked and the range (corners in slide points), the
/// cell being edited with its text and look (canvas.rs cell_editor).
fn table_overlay(app: &App, sc: &SlideScene) -> Value {
    let Some((id, r, c)) = super::table_cursor(app).or_else(|| app.canvas.table_range.map(|r| (r.0, r.1, r.2))) else { return Value::Null };
    let Some(d) = sc.drawables.iter().find(|d| d.id == id) else { return Value::Null };
    let DrawableKind::Table(t) = &d.kind else { return Value::Null };
    let f = t.frame(&d.geom);
    let quad = |r0: u32, c0: u32, r1: u32, c1: u32| {
        let (x0, y0, _, _) = super::cell_rect(&t.row_heights, &t.col_widths, r0, c0);
        let (x1, y1, w1, h1) = super::cell_rect(&t.row_heights, &t.col_widths, r1, c1);
        json!([pt(f.to_slide(Point::new(x0, y0))), pt(f.to_slide(Point::new(x1 + w1, y0))), pt(f.to_slide(Point::new(x1 + w1, y1 + h1))), pt(f.to_slide(Point::new(x0, y1 + h1)))])
    };
    let range = app.canvas.table_range.filter(|r| r.0 == id).map(|(_, r0, c0, r1, c1)| quad(r0, c0, r1, c1));
    let editing = app.canvas.cell.as_ref().filter(|e| e.0 == id).map(|(_, er, ec, text, _)| {
        let style = t.cell_styles.get((er * t.cols + ec) as usize).cloned().unwrap_or_default();
        let col = |k: kn_model::Color| json!([k.r, k.g, k.b, k.a]);
        let back = match &style.fill {
            kn_model::Fill::Color(k) if k.a > 0.99 => *k,
            kn_model::Fill::Gradient { stops, .. } if !stops.is_empty() => stops[0].1,
            _ => kn_model::Color::WHITE,
        };
        json!({
            "quad": quad(*er, *ec, *er, *ec), "row": er, "col": ec, "text": text,
            "size": if style.text.size > 0.0 { style.text.size } else { 20.0 },
            "color": col(if style.text.color.a <= 0.0 { kn_model::Color::BLACK } else { style.text.color }),
            "fill": col(back),
            "align": match style.align { kn_model::Align::Center => 1, kn_model::Align::Right => 2, _ => 0 },
            "padding": style.padding,
        })
    });
    json!({ "id": id, "cell": quad(r, c, r, c), "row": r, "col": c, "rows": t.rows, "cols": t.cols, "range": range, "editing": editing })
}

fn pen_drag(app: &mut App, sc: &SlideScene, kind: u32, p: Point) {
    match kind {
        1 => app.pen = Some(vec![p]),
        2 => {
            let pts = app.pen.replace(Vec::new()).unwrap_or_default();
            if pts.len() > 1 {
                let c = app.pen_color;
                let col = kn_model::Color { r: c.r() as f32 / 255.0, g: c.g() as f32 / 255.0, b: c.b() as f32 / 255.0, a: 1.0 };
                let width = app.pen_width;
                if let Some(doc) = app.doc.as_mut() {
                    doc.begin(&mut app.hist, "Draw");
                    let _ = doc.insert_freehand(&mut app.hist, sc.slide, &pts, col, width);
                    doc.end(&mut app.hist);
                }
                app.bump(sc.slide);
            }
        }
        _ => {
            if let Some(pts) = app.pen.as_mut() {
                if pts.last().is_none_or(|q| ((q.x - p.x).powi(2) + (q.y - p.y).powi(2)).sqrt() > 1.5) {
                    pts.push(p);
                }
            }
        }
    }
}

/// The node or handle under `p` (sub-path, node, 0 node / 1 in / 2 out).
fn point_hit(app: &App, sc: &SlideScene, p: Point, tol: f32) -> Option<(usize, usize, u8)> {
    let pe = app.point_edit.as_ref()?;
    let g = sc.drawables.iter().find(|d| d.id == pe.id)?.geom;
    let mut best: Option<((usize, usize, u8), f32)> = None;
    for (si, sp) in pe.nodes.subpaths.iter().enumerate() {
        for (i, n) in sp.nodes.iter().enumerate() {
            for (part, q) in [(0u8, n.pt), (1, n.inc), (2, n.out)] {
                if part > 0 && pe.selected != Some((si, i)) && n.kind == kn_model::points::NodeKind::Sharp {
                    continue;
                }
                let s = g.to_slide(q);
                let dd = ((s.x - p.x).powi(2) + (s.y - p.y).powi(2)).sqrt();
                if dd < tol && best.is_none_or(|b| dd < b.1) {
                    best = Some(((si, i, part), dd));
                }
            }
        }
    }
    best.map(|b| b.0)
}

fn point_drag(app: &mut App, sc: &SlideScene, kind: u32, p: Point, tol: f32) {
    use kn_model::points::NodeKind;
    let Some(g) = app.point_edit.as_ref().and_then(|pe| sc.drawables.iter().find(|d| d.id == pe.id)).map(|d| d.geom) else { return };
    match kind {
        1 => {
            let h = point_hit(app, sc, p, tol);
            if let Some(pe) = app.point_edit.as_mut() {
                pe.drag = h;
                if let Some(h) = h {
                    pe.selected = Some((h.0, h.1));
                }
            }
        }
        2 => {
            let Some(pe) = app.point_edit.as_mut() else { return };
            if pe.drag.take().is_some() {
                let (id, nodes) = (pe.id, pe.nodes.clone());
                if let Some(doc) = app.doc.as_mut() {
                    doc.begin(&mut app.hist, "Edit Points");
                    doc.set_editable_nodes(&mut app.hist, id, &nodes);
                    doc.end(&mut app.hist);
                }
                app.bump(sc.slide);
            }
        }
        _ => {
            let Some(pe) = app.point_edit.as_mut() else { return };
            let Some((si, i, part)) = pe.drag else { return };
            let q = g.to_local(p);
            let Some(n) = pe.nodes.subpaths.get_mut(si).and_then(|s| s.nodes.get_mut(i)) else { return };
            match part {
                0 => {
                    let (dx, dy) = (q.x - n.pt.x, q.y - n.pt.y);
                    n.pt = q;
                    n.inc = Point::new(n.inc.x + dx, n.inc.y + dy);
                    n.out = Point::new(n.out.x + dx, n.out.y + dy);
                }
                _ => {
                    if part == 1 { n.inc = q } else { n.out = q }
                    if n.kind == NodeKind::Smooth {
                        let (h, o) = if part == 1 { (n.inc, &mut n.out) } else { (n.out, &mut n.inc) };
                        let len = ((o.x - n.pt.x).powi(2) + (o.y - n.pt.y).powi(2)).sqrt();
                        let (dx, dy) = (n.pt.x - h.x, n.pt.y - h.y);
                        let l = (dx * dx + dy * dy).sqrt().max(1e-3);
                        *o = Point::new(n.pt.x + dx / l * len, n.pt.y + dy / l * len);
                    } else if n.kind == NodeKind::Sharp {
                        n.kind = NodeKind::Bezier;
                    }
                }
            }
        }
    }
}

/// Make Editable: the outline as edited (sampled), its nodes and the selected node's handles,
/// in slide points.
fn points_overlay(app: &App, sc: &SlideScene) -> Value {
    let Some(pe) = app.point_edit.as_ref().filter(|pe| app.sel == [pe.id]) else { return Value::Null };
    let Some(g) = sc.drawables.iter().find(|d| d.id == pe.id).map(|d| d.geom) else { return Value::Null };
    let to = |p: Point| pt(g.to_slide(p));
    let mut lines = Vec::new();
    let mut nodes = Vec::new();
    for (si, sp) in pe.nodes.subpaths.iter().enumerate() {
        let n = sp.nodes.len();
        let segs = if sp.closed { n } else { n.saturating_sub(1) };
        let mut line = Vec::new();
        for i in 0..segs {
            let (a, b) = (sp.nodes[i], sp.nodes[(i + 1) % n]);
            for k in 0..=16 {
                let t = k as f32 / 16.0;
                let u = 1.0 - t;
                let x = u * u * u * a.pt.x + 3.0 * u * u * t * a.out.x + 3.0 * u * t * t * b.inc.x + t * t * t * b.pt.x;
                let y = u * u * u * a.pt.y + 3.0 * u * u * t * a.out.y + 3.0 * u * t * t * b.inc.y + t * t * t * b.pt.y;
                line.push(to(Point::new(x, y)));
            }
        }
        lines.push(line);
        for (i, nd) in sp.nodes.iter().enumerate() {
            let on = pe.selected == Some((si, i));
            let handles = on || nd.kind != kn_model::points::NodeKind::Sharp;
            nodes.push(json!({ "at": to(nd.pt), "on": on, "in": handles.then(|| to(nd.inc)), "out": handles.then(|| to(nd.out)) }));
        }
    }
    json!({ "lines": lines, "nodes": nodes })
}

/// Alignment-guide options from Settings > Rulers (canvas.rs snap_opts): within about 6 screen
/// points, here a quarter of a finger's reach.
fn snap_opts(s: &crate::settings::Settings, tol: f32, off: bool) -> crate::snap::Opts {
    crate::snap::Opts { center: s.guides_center && !off, edges: s.guides_edges && !off, spacing: s.guides_spacing && !off, sizing: s.guides_sizing && !off, tol: tol * 0.27 }
}

/// Guides (slide points): the slide's own ([vertical, pos]), the layout's, the one being dragged,
/// and the alignment guides of a move or resize (["line", a, b], ["gap", axis, from, to, at],
/// ["size", axis, [x0, y0, x1, y1]]); whether the rulers show, in which units.
fn guides_overlay(app: &App, sc: &SlideScene) -> Value {
    use crate::snap::Guide;
    let Some(doc) = app.doc.as_ref() else { return Value::Null };
    let moving = match &app.canvas.drag {
        Some(Drag::Guide { index, vertical, pos }) => Some((*index, *vertical, *pos)),
        _ => None,
    };
    let user: Vec<Value> = if app.show_guides { doc.guides(sc.slide).into_iter().enumerate().filter(|(i, _)| moving.is_none_or(|m| m.0 != Some(*i))).map(|(_, g)| json!([g.vertical, g.pos])).collect() } else { Vec::new() };
    let layout: Vec<Value> = if app.show_layout_guides { doc.layout_guides(sc.slide).into_iter().map(|g| json!([g.vertical, g.pos])).collect() } else { Vec::new() };
    let snap: Vec<Value> = app
        .canvas
        .guides
        .iter()
        .map(|g| match g {
            Guide::Line(a, b) => json!(["line", pt(*a), pt(*b)]),
            Guide::Gap { axis, from, to, at } => json!(["gap", axis, from, to, at]),
            Guide::Size { axis, bx } => json!(["size", axis, [bx.x0, bx.y0, bx.x1, bx.y1]]),
        })
        .collect();
    json!({ "user": user, "layout": layout, "dragging": moving.map(|m| json!([m.1, m.2])), "snap": snap, "rulers": app.show_rulers, "units": app.settings.ruler_units })
}
