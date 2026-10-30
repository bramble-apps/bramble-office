//! Format > Chart > Edit Chart Data…: the suite's editor (kn_ui::chartdata), which follows
//! the chart's type and series direction; each change is one undo step.

use nm_core::kn_format::ObjId;
use nm_core::kn_model::{ChartData, DrawableKind};

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
