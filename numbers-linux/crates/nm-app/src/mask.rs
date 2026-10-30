//! Format > Image: Edit Mask (a bar under the mask with the picture's size and Done; dragging
//! moves the picture under the mask), Auto Crop, Reset Mask and Mask with Shape. Keydeck's,
//! on Values' canvas.

use egui::Pos2;
use nm_core::kn_model::{DrawableKind, Geometry};

use crate::app::App;

/// The mask of picture `id` in sheet points (the whole picture while it is being masked for
/// the first time); None for anything else or a rotated picture.
pub fn mask_box(app: &App, id: nm_core::kn_format::ObjId) -> Option<Geometry> {
    let d = app.drawables.iter().find(|d| d.id == id)?;
    let DrawableKind::Image(i) = &d.kind else { return None };
    if d.geom.angle.rem_euclid(360.0) != 0.0 {
        return None;
    }
    match i.mask.as_ref() {
        Some((m, _)) => Some(Geometry { x: d.geom.x + m.x, y: d.geom.y + m.y, w: m.w, h: m.h, ..d.geom }),
        None if app.mask_edit == Some(id) => Some(d.geom),
        None => None,
    }
}

/// The selected picture.
pub fn selected_image(app: &App) -> Option<nm_core::kn_format::ObjId> {
    let o = app.canvas.obj?;
    app.drawables.iter().any(|d| d.id == o && matches!(d.kind, DrawableKind::Image(_))).then_some(o)
}

/// Edit Mask's bar under the mask (screen point `at`): the picture's size (it grows and shrinks
/// about the mask's centre, the mask staying put) and Done.
pub fn mask_bar(app: &mut App, ctx: &egui::Context, at: Pos2) {
    let Some(id) = app.mask_edit else { return };
    let (Some(m), Some(g)) = (mask_box(app, id), app.drawables.iter().find(|d| d.id == id).map(|d| d.geom)) else { return };
    let mut pct = (g.w / m.w.max(0.01) * 100.0).clamp(100.0, 500.0);
    let (mut done, mut release) = (false, false);
    egui::Area::new(egui::Id::unique(("mask-bar", id))).order(egui::Order::Foreground).pivot(egui::Align2::CENTER_TOP).fixed_pos(at).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).corner_radius(10).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().slider_width = 160.0;
                let r = ui.add(egui::Slider::new(&mut pct, 100.0..=500.0).show_value(false)).on_hover_text("Picture size");
                release = r.drag_stopped() || (r.changed() && !r.dragged());
                if ui.add(egui::Button::new("Done").fill(kn_ui::theme::ACCENT())).clicked() {
                    done = true;
                }
            });
        });
    });
    if release {
        let k = (m.w * pct / 100.0) / g.w.max(0.01);
        let (cx, cy) = (m.x + m.w / 2.0, m.y + m.h / 2.0);
        let ng = Geometry { x: cx - (cx - g.x) * k, y: cy - (cy - g.y) * k, w: g.w * k, h: g.h * k, ..g };
        app.edit("Edit Mask", |d, h| {
            d.set_frame(h, id, ng);
            d.set_mask_rect(h, id, m.x - ng.x, m.y - ng.y, m.w, m.h);
        });
    }
    if done {
        app.mask_edit = None;
    }
}

/// Dragging a picture whose mask is being edited moves the picture under the mask.
pub fn move_under_mask(app: &mut App, id: nm_core::kn_format::ObjId, dx: f32, dy: f32) {
    let Some(m) = mask_box(app, id) else { return };
    let Some(g) = app.drawables.iter().find(|d| d.id == id).map(|d| d.geom) else { return };
    app.edit("Edit Mask", |d, h| {
        d.move_by(h, id, dx, dy);
        d.set_mask_rect(h, id, m.x - (g.x + dx), m.y - (g.y + dy), m.w, m.h);
    });
}

/// Format > Image > Auto Crop: the picture masked to its content, without the even border (or
/// transparent margin) around it.
pub fn auto_crop(app: &mut App) {
    let Some(id) = selected_image(app) else { return };
    let Some((data, gw, gh)) = app.drawables.iter().find(|d| d.id == id).and_then(|d| match &d.kind {
        DrawableKind::Image(i) => Some((i.data_id?, d.geom.w, d.geom.h)),
        _ => None,
    }) else {
        return;
    };
    let Some(img) = app.doc().and_then(|doc| doc.data(data)).and_then(|b| image::load_from_memory(&b).ok()).map(|i| i.to_rgba8()) else {
        app.notify("Couldn't read the picture");
        return;
    };
    let (w, h) = img.dimensions();
    let corner = *img.get_pixel(0, 0);
    let differs = |p: &image::Rgba<u8>| {
        if corner[3] < 16 {
            p[3] >= 16
        } else {
            (0..3).any(|k| (i32::from(p[k]) - i32::from(corner[k])).abs() > 30) || p[3] < 200
        }
    };
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0u32, 0u32);
    for (x, y, p) in img.enumerate_pixels() {
        if differs(p) {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x + 1);
            y1 = y1.max(y + 1);
        }
    }
    if x1 <= x0 || y1 <= y0 || (x0, y0, x1, y1) == (0, 0, w, h) {
        app.notify("There's no border to crop");
        return;
    }
    let (fx, fy) = (gw / w as f32, gh / h as f32);
    app.edit("Auto Crop", |d, hh| d.set_mask_rect(hh, id, x0 as f32 * fx, y0 as f32 * fy, (x1 - x0) as f32 * fx, (y1 - y0) as f32 * fy));
}
