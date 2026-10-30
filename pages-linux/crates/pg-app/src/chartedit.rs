//! Format > Chart > Edit Chart Data…: the suite's editor (kn_ui::chartdata), which follows
//! the chart's type and series direction; each change is one undo step.

use pg_core::kn_format::ObjId;
use pg_core::kn_model::{ChartData, DrawableKind};

use crate::app::App;

pub use kn_ui::chartdata::ChartDataEditor as ChartEdit;

/// The chart as drawn (its series colours).
fn chart_data(app: &App, id: ObjId) -> Option<ChartData> {
    app.drawables.iter().find(|d| d.id == id).and_then(|d| match &d.kind {
        DrawableKind::Chart(c) => Some(c.clone()),
        _ => None,
    })
}

pub fn open(app: &mut App, chart: ObjId) {
    let c = chart_data(app, chart);
    if let Some(ed) = app.doc().and_then(|d| ChartEdit::open(d, chart, c.as_ref())) {
        app.chart_edit = Some(ed);
    }
}

pub fn ui(app: &mut App, ctx: &egui::Context) {
    let Some(mut ed) = app.chart_edit.take() else { return };
    let (open, act) = ed.window(ctx);
    if act != kn_ui::chartdata::DataAction::None {
        let id = ed.chart;
        app.edit(act.undo_name(), |d, h| act.apply(d, h, id));
        let c = chart_data(app, id);
        if let Some(d) = app.doc() {
            ed.refresh(d, c.as_ref());
        }
    }
    if open {
        app.chart_edit = Some(ed);
    }
}

/// Pages / Keynote: a selected chart shows an "Edit Chart Data" button just below it. True
/// while the pointer is the button's.
pub fn canvas_button(app: &mut App, ui: &mut egui::Ui) -> bool {
    if app.chart_edit.is_some() || app.canvas.dragging() {
        return false;
    }
    let [id] = app.selected_objs()[..] else { return false };
    let Some(d) = app.drawables.iter().find(|d| d.id == id && matches!(d.kind, pg_core::kn_model::DrawableKind::Chart(_))).cloned() else { return false };
    let (Some(l), Some(pi)) = (app.layout.as_ref(), app.obj_page.get(&id).copied()) else { return false };
    let g = d.geom;
    let bottom = app.canvas.page_to_screen(l, pi, egui::pos2(g.x + g.w / 2.0, g.y + g.h));
    let r = egui::Rect::from_center_size(bottom + egui::vec2(0.0, 22.0 * app.canvas.zoom + 24.0), egui::vec2(140.0, 26.0));
    if !app.canvas.view_rect().intersects(r) {
        return false;
    }
    let resp = ui.interact(r, ui.scope_id().with(("edit-chart-data", id)), egui::Sense::click());
    let p = ui.painter();
    let (fill, ink) = if resp.hovered() { (kn_ui::theme::ACCENT(), egui::Color32::WHITE) } else { (egui::Color32::WHITE, kn_ui::theme::ACCENT()) };
    p.rect_filled(r.translate(egui::vec2(0.0, 1.5)), 13.0, egui::Color32::from_black_alpha(30));
    p.rect(r, 13.0, fill, egui::Stroke::new(1.0, kn_ui::theme::ACCENT().gamma_multiply(0.6)), egui::StrokeKind::Inside);
    p.text(r.center(), egui::Align2::CENTER_CENTER, "Edit Chart Data", egui::FontId::proportional(12.5), ink);
    if resp.clicked() {
        open(app, id);
    }
    resp.hovered()
}
