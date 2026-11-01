//! Format > Shapes and Lines: Make Editable and editing a shape's points on the canvas (drag a
//! point or one of its handles; Make Sharp / Smooth / Bézier Point for the selected one).
//! Keydeck's, through kn-model's editable bezier nodes.

use egui::{Color32, Pos2, Rect, Stroke, Vec2};
use pg_core::kn_format::ObjId;
use pg_core::kn_model::points::{Editable, NodeKind};
use pg_core::kn_model::{DrawableKind, Point};

use crate::app::App;

/// The point being edited: shape, selected node (subpath, index), and a drag in progress
/// (subpath, index, part: 0 the point, 1 its in handle, 2 its out handle; where it started).
#[derive(Clone, Copy, Default)]
pub struct PointEdit {
    pub id: Option<ObjId>,
    pub selected: Option<(usize, usize)>,
    pub drag: Option<(usize, usize, u8, Pos2)>,
}

/// Format > Shapes and Lines > Make Editable: the shape's outline as editable points, shown
/// for editing.
pub fn make_editable(app: &mut App) {
    let Some(id) = app.canvas.obj else { return };
    let Some(outline) = app.drawables.iter().find(|d| d.id == id).and_then(|d| match &d.kind {
        DrawableKind::Shape(s) => Some(s.path.clone()),
        _ => None,
    }) else {
        return;
    };
    app.edit("Make Editable", |d, h| d.make_editable(h, id, &outline));
    app.points = PointEdit { id: Some(id), selected: None, drag: None };
}

/// Make Sharp / Smooth / Bézier Point on the selected point.
pub fn set_kind(app: &mut App, kind: NodeKind) {
    let (Some(id), Some((s, i))) = (app.points.id, app.points.selected) else { return };
    let Some(mut e) = app.doc().and_then(|d| d.editable_nodes(id)) else { return };
    e.set_kind(s, i, kind);
    app.edit("Point Type", |d, h| d.set_editable_nodes(h, id, &e));
}

/// Shapes and Lines > Toggle Line Type: every point of the shape being edited becomes a curve,
/// or (when any already is) a corner.
pub fn toggle_line_type(app: &mut App) {
    let Some(id) = app.points.id else { return };
    let Some(mut e) = app.doc().and_then(|d| d.editable_nodes(id)) else { return };
    let any_curve = e.subpaths.iter().flat_map(|s| s.nodes.iter()).any(|n| n.kind != NodeKind::Sharp);
    let k = if any_curve { NodeKind::Sharp } else { NodeKind::Smooth };
    for si in 0..e.subpaths.len() {
        for i in 0..e.subpaths[si].nodes.len() {
            e.set_kind(si, i, k);
        }
    }
    app.edit("Line Type", |d, h| d.set_editable_nodes(h, id, &e));
}

fn nodes(app: &App) -> Option<(ObjId, Editable, pg_core::kn_model::Geometry)> {
    let id = app.points.id?;
    let g = app.drawables.iter().find(|d| d.id == id)?.geom;
    let e = app.doc()?.editable_nodes(id)?;
    Some((id, e, g))
}

/// Draw the points of the shape being edited and handle dragging them. `to_screen` maps sheet
/// points to the screen. Returns true when the pointer was used (the canvas leaves it alone).
pub fn canvas(app: &mut App, ui: &egui::Ui, resp: &egui::Response, to_screen: &dyn Fn(Pos2) -> Pos2, zoom: f32) -> bool {
    // editing stops when something else is selected
    if app.points.id.is_some() && app.canvas.obj != app.points.id {
        app.points = PointEdit::default();
    }
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.points = PointEdit::default();
    }
    let Some((id, mut e, g)) = nodes(app) else { return false };
    if g.angle.rem_euclid(360.0) != 0.0 {
        return false;
    }
    let at = |p: Point| to_screen(Pos2::new(g.x + p.x, g.y + p.y));
    let painter = ui.painter();
    let accent = kn_ui::theme::ACCENT();
    // where the pointer is over: (subpath, index, part)
    // a drag starts where the button went down (the pointer has moved on by then)
    let hover = if resp.drag_started() { ui.input(|i| i.pointer.press_origin()) } else { ui.input(|i| i.pointer.hover_pos()) };
    let mut over: Option<(usize, usize, u8)> = None;
    for (s, sp) in e.subpaths.iter().enumerate() {
        for (i, n) in sp.nodes.iter().enumerate() {
            let sel = app.points.selected == Some((s, i));
            if sel && n.kind != NodeKind::Sharp {
                for (k, c) in [(1u8, n.inc), (2u8, n.out)] {
                    painter.line_segment([at(n.pt), at(c)], Stroke::new(1.0, accent));
                    painter.circle(at(c), 3.5, Color32::WHITE, Stroke::new(1.0, accent));
                    if hover.is_some_and(|h| h.distance(at(c)) <= 6.0) {
                        over = Some((s, i, k));
                    }
                }
            }
            let fill = if sel { accent } else { Color32::WHITE };
            if n.kind == NodeKind::Sharp {
                painter.rect(Rect::from_center_size(at(n.pt), Vec2::splat(7.0)), 0.0, fill, Stroke::new(1.0, accent), egui::StrokeKind::Middle);
            } else {
                painter.circle(at(n.pt), 4.0, fill, Stroke::new(1.0, accent));
            }
            if over.is_none() && hover.is_some_and(|h| h.distance(at(n.pt)) <= 6.0) {
                over = Some((s, i, 0));
            }
        }
    }
    let mut used = over.is_some();
    // press: select the point; drag: move it (or its handle)
    if resp.drag_started()
        && let (Some((s, i, k)), Some(p)) = (over, ui.input(|i| i.pointer.press_origin()))
    {
        app.points.selected = Some((s, i));
        app.points.drag = Some((s, i, k, p));
    } else if resp.clicked()
        && let Some((s, i, _)) = over
    {
        app.points.selected = Some((s, i));
    }
    if let (Some((s, i, k, from)), Some(p)) = (app.points.drag, ui.input(|i| i.pointer.latest_pos())) {
        used = true;
        let d = (p - from) / zoom;
        let n = &mut e.subpaths[s].nodes[i];
        let shift = |q: &mut Point| {
            q.x += d.x;
            q.y += d.y;
        };
        match k {
            0 => {
                shift(&mut n.pt);
                shift(&mut n.inc);
                shift(&mut n.out);
            }
            1 | 2 => {
                let (moved, other) = if k == 1 { (&mut n.inc, &mut n.out) } else { (&mut n.out, &mut n.inc) };
                shift(moved);
                // a smooth point keeps its handles in line
                if n.kind == NodeKind::Smooth {
                    let m = *moved;
                    *other = Point::new(2.0 * n.pt.x - m.x, 2.0 * n.pt.y - m.y);
                }
            }
            _ => {}
        }
        let n = e.subpaths[s].nodes[i];
        painter.circle(at(n.pt), 4.5, accent, Stroke::new(1.0, Color32::WHITE));
        if resp.drag_stopped() {
            app.points.drag = None;
            commit(app, id, e, g);
        }
    }
    used
}

/// The edited points stored as one undo step; the frame follows the outline (Pages keeps the
/// frame around the points).
fn commit(app: &mut App, id: ObjId, mut e: Editable, g: pg_core::kn_model::Geometry) {
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for sp in &e.subpaths {
        for n in &sp.nodes {
            for q in [n.pt, n.inc, n.out] {
                (x0, y0, x1, y1) = (x0.min(q.x), y0.min(q.y), x1.max(q.x), y1.max(q.y));
            }
        }
    }
    let fit = x1 > x0 && y1 > y0;
    if fit {
        for sp in &mut e.subpaths {
            for n in &mut sp.nodes {
                for q in [&mut n.pt, &mut n.inc, &mut n.out] {
                    q.x -= x0;
                    q.y -= y0;
                }
            }
        }
    }
    let ng = pg_core::kn_model::Geometry { x: g.x + x0, y: g.y + y0, w: x1 - x0, h: y1 - y0, ..g };
    app.edit("Edit Points", |doc, h| {
        if fit {
            doc.set_frame(h, id, ng);
        }
        doc.set_editable_nodes(h, id, &e)
    });
}

/// The points being edited for a front end (canvas): the shape, its page, the frame's origin
/// (page points), the points [subpath, index, kind (0 sharp, 1 smooth, 2 bézier), x, y, in x,
/// in y, out x, out y] (relative to the origin) and the selected one; null when none.
pub fn api_state(app: &mut App) -> serde_json::Value {
    use serde_json::json;
    if app.points.id.is_some() && app.canvas.obj != app.points.id {
        app.points = PointEdit::default();
    }
    let Some((id, e, g)) = nodes(app) else { return serde_json::Value::Null };
    if g.angle.rem_euclid(360.0) != 0.0 {
        return serde_json::Value::Null;
    }
    let page = app.obj_page.get(&id).copied().unwrap_or(0);
    let mut v = Vec::new();
    for (s, sp) in e.subpaths.iter().enumerate() {
        for (i, n) in sp.nodes.iter().enumerate() {
            let kind = match n.kind {
                NodeKind::Sharp => 0,
                NodeKind::Smooth => 1,
                _ => 2,
            };
            v.push(json!([s, i, kind, n.pt.x, n.pt.y, n.inc.x, n.inc.y, n.out.x, n.out.y]));
        }
    }
    json!({ "id": id, "page": page, "origin": [g.x, g.y], "nodes": v, "selected": app.points.selected.map(|(s, i)| json!([s, i])) })
}

/// A front end's point actions (canvas): "sel:<s>:<i>" selects a point; "move:<s>:<i>:<part>:
/// <dx>:<dy>" moves it (part 0) or its in / out handle (1 / 2) by page points, one undo step;
/// "stop" ends the editing.
pub fn action(app: &mut App, arg: &str) {
    let p: Vec<&str> = arg.split(':').collect();
    let num = |k: usize| p.get(k).and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
    match p.first().copied() {
        Some("sel") => app.points.selected = Some((num(1) as usize, num(2) as usize)),
        Some("move") => {
            let Some((id, mut e, g)) = nodes(app) else { return };
            let (s, i, k) = (num(1) as usize, num(2) as usize, num(3) as u8);
            let Some(n) = e.subpaths.get_mut(s).and_then(|sp| sp.nodes.get_mut(i)) else { return };
            let d = Point::new(num(4), num(5));
            let shift = |q: &mut Point| {
                q.x += d.x;
                q.y += d.y;
            };
            match k {
                0 => {
                    shift(&mut n.pt);
                    shift(&mut n.inc);
                    shift(&mut n.out);
                }
                _ => {
                    let (moved, other) = if k == 1 { (&mut n.inc, &mut n.out) } else { (&mut n.out, &mut n.inc) };
                    shift(moved);
                    if n.kind == NodeKind::Smooth {
                        let m = *moved;
                        *other = Point::new(2.0 * n.pt.x - m.x, 2.0 * n.pt.y - m.y);
                    }
                }
            }
            app.points.selected = Some((s, i));
            commit(app, id, e, g);
        }
        Some("stop") => app.points = PointEdit::default(),
        _ => {}
    }
}
