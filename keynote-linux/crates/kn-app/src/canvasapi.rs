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
    match kind {
        1 => {
            if app.canvas.editing.is_some() {
                crate::textedit::commit(app);
            }
            app.canvas.drag = None;
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
            Some(Drag::Move { start, delta }) => *delta = (p.x - start.x, p.y - start.y),
            Some(Drag::Resize { handle, orig, cur, .. }) => {
                let keep = shift || sc.drawables.iter().any(|d| app.sel.contains(&d.id) && matches!(d.kind, DrawableKind::Image(_)));
                *cur = resized(orig, *handle, orig.to_local(p), keep);
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
        if Some(id) == editing_id {
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
    json!({
        "prompts": prompts,
        "selection": sel,
        "editing": editing,
        "rubber": rubber,
        "dragging": dragging,
        "moving": moving(app),
        "size": [sc.size.w, sc.size.h],
    })
}
