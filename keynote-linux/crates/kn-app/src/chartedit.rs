//! "Edit Chart Data" window: the suite's editor (kn_ui::chartdata), which follows the chart's
//! type and series direction; each change is one undo step.

use kn_format::ObjId;
use kn_model::{ChartData, DrawableKind};

use crate::app::App;

pub use kn_ui::chartdata::ChartDataEditor as ChartEdit;

/// The chart as drawn on the current slide (its series colours).
fn chart_data(app: &mut App, id: ObjId) -> Option<ChartData> {
    app.scene(app.cur)?.drawables.into_iter().find(|d| d.id == id).and_then(|d| match d.kind {
        DrawableKind::Chart(c) => Some(c),
        _ => None,
    })
}

pub fn open(app: &mut App, chart: ObjId) {
    let c = chart_data(app, chart);
    if let Some(ed) = app.doc.as_ref().and_then(|d| ChartEdit::open(d, chart, c.as_ref())) {
        app.chart_edit = Some(ed);
    }
}

pub fn ui(app: &mut App, ctx: &egui::Context) {
    let Some(mut ed) = app.chart_edit.take() else { return };
    let (open, act) = ed.window(ctx);
    if act != kn_ui::chartdata::DataAction::None {
        let slide = app.current_slide().map(|s| s.slide);
        if let Some(doc) = app.doc.as_mut() {
            doc.begin(&mut app.hist, act.undo_name());
            act.apply(doc, &mut app.hist, ed.chart);
            doc.end(&mut app.hist);
        }
        if let Some(s) = slide {
            app.bump(s);
        }
        let c = chart_data(app, ed.chart);
        if let Some(d) = app.doc.as_ref() {
            ed.refresh(d, c.as_ref());
        }
    }
    if open {
        app.chart_edit = Some(ed);
    }
}
