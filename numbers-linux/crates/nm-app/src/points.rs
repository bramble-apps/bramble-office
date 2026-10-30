//! Format > Shapes and Lines: Make Editable and editing a shape's points on the canvas (drag a
//! point or one of its handles; Make Sharp / Smooth / Bézier Point for the selected one).
//! Keydeck's, through kn-model's editable bezier nodes.

use egui::{Color32, Pos2, Rect, Stroke, Vec2};
use nm_core::kn_format::ObjId;
use nm_core::kn_model::points::{Editable, NodeKind};
use nm_core::kn_model::{DrawableKind, Point};

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

fn nodes(app: &App) -> Option<(ObjId, Editable, nm_core::kn_model::Geometry)> {
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
    let hover = ui.input(|i| i.pointer.hover_pos());
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
            app.edit("Edit Points", |doc, h| doc.set_editable_nodes(h, id, &e));
        }
    }
    used
}
