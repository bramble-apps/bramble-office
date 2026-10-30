//! Edit Data References (Numbers): a chart linked to table cells shows its cells outlined in its
//! series' colours; selecting other cells and pressing Done links the chart to them, plotting the
//! table's columns or rows as series. Escape (or Cancel) leaves the link as it was.

use egui::{Color32, Pos2, Rect, RichText};
use kn_ui::theme;
use nm_core::kn_format::ObjId;
use nm_core::kn_model::DrawableKind;

use crate::app::{App, Sel};

/// The mode: the chart being re-referenced, and whether the table's rows are its series.
#[derive(Clone, Copy, Debug)]
pub struct DataRefs {
    pub chart: ObjId,
    pub rows_are_series: bool,
}

/// The chart's link, when it's linked to a table of this document.
pub fn linked(app: &App, chart: ObjId) -> Option<nm_core::chartlink::ChartLink> {
    nm_core::chartlink::link(app.doc()?, chart).filter(|l| l.table.is_some())
}

/// Start editing `chart`'s references: its table shown, its cells selected.
pub fn start(app: &mut App, chart: ObjId) {
    let Some(l) = linked(app, chart) else { return };
    let t = l.table.unwrap();
    crate::canvas::commit(app);
    if let Some(i) = app.sheets().iter().position(|s| s.drawables.contains(&t)) {
        if i != app.sheet {
            app.sheet = i;
            app.canvas.reset();
            app.refresh();
        }
    }
    let rs = l.ranges();
    let (r0, c0) = (rs.iter().map(|r| r.0).min().unwrap_or(0), rs.iter().map(|r| r.1).min().unwrap_or(0));
    let (r1, c1) = (rs.iter().map(|r| r.2).max().unwrap_or(0), rs.iter().map(|r| r.3).max().unwrap_or(0));
    app.canvas.obj = None;
    app.canvas.multi.clear();
    app.sel = Some(Sel { table: t, anchor: (r0, c0), cursor: (r1, c1) });
    app.reveal_cell(t, r0, c0);
    app.reveal_cell(t, r1, c1);
    app.data_refs = Some(DataRefs { chart, rows_are_series: !l.columns_are_series() });
}

/// Outlines of the chart's cells: (table, block, colour); series in their colours, labels grey.
pub fn outlines(app: &App) -> Vec<(ObjId, (u32, u32, u32, u32), Color32)> {
    let Some(m) = app.data_refs else { return Vec::new() };
    let Some(l) = linked(app, m.chart) else { return Vec::new() };
    let t = l.table.unwrap();
    let colors: Vec<Color32> = app
        .wb
        .as_ref()
        .and_then(|w| w.sheets().into_iter().find_map(|s| s.drawables.contains(&m.chart).then_some(s)).map(|s| w.drawables_and_views(&s).0))
        .and_then(|ds| {
            ds.into_iter().find(|d| d.id == m.chart).and_then(|d| match d.kind {
                DrawableKind::Chart(c) => Some(c.series.iter().map(|s| Color32::from_rgb((s.color.r * 255.0) as u8, (s.color.g * 255.0) as u8, (s.color.b * 255.0) as u8)).collect()),
                _ => None,
            })
        })
        .unwrap_or_default();
    let mut out: Vec<(ObjId, (u32, u32, u32, u32), Color32)> = l.series.iter().enumerate().map(|(i, b)| (t, *b, colors.get(i).copied().unwrap_or(theme::ACCENT()))).collect();
    for &(r, c) in l.row_labels.iter().chain(&l.col_labels) {
        out.push((t, (r, c, r, c), Color32::from_gray(0x90)));
    }
    out
}

/// Link the chart to the selected cells.
fn done(app: &mut App) {
    let Some(m) = app.data_refs.take() else { return };
    let Some(sel) = app.sel else { return };
    let Some((_, t)) = app.table(sel.table) else { return };
    if app.views.contains_key(&sel.table) {
        app.notify("A table shown in categories can't be charted cell by cell; turn categories off first");
        return;
    }
    let b = sel.block();
    let (series, names, cats) = if m.rows_are_series { nm_core::charts::link_cells_by_row(t, b) } else { nm_core::charts::link_cells(t, b) };
    if series.is_empty() {
        app.notify("Select cells with numbers to plot");
        app.data_refs = Some(m);
        return;
    }
    let table = sel.table;
    app.edit("Edit Data References", |d, h| {
        nm_core::chartlink::set_link(d, h, m.chart, table, &series, &names, &cats, !m.rows_are_series);
        nm_core::chartlink::refresh(d, h);
    });
    select_chart(app, m.chart);
}

fn select_chart(app: &mut App, chart: ObjId) {
    if let Some(i) = app.sheets().iter().position(|s| s.drawables.contains(&chart)) {
        if i != app.sheet {
            app.sheet = i;
            app.canvas.reset();
            app.refresh();
        }
    }
    app.sel = None;
    app.canvas.obj = Some(chart);
}

/// The bar at the bottom of the sheet while editing references.
pub fn bar(app: &mut App, ctx: &egui::Context, sheet_rect: Rect) {
    let Some(mut m) = app.data_refs else { return };
    let mut finish = None;
    egui::Area::new(egui::Id::unique("data-refs-bar"))
        .order(egui::Order::Foreground)
        .fixed_pos(Pos2::new(sheet_rect.center().x - 260.0, sheet_rect.max.y - 64.0))
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).corner_radius(12.0).inner_margin(egui::Margin::symmetric(14, 10)).show(ui, |ui| {
                ui.set_width(520.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Edit Data References").strong());
                    ui.add_space(8.0);
                    let mut i = usize::from(m.rows_are_series);
                    ui.allocate_ui(egui::vec2(250.0, 28.0), |ui| crate::objinsp::seg_small(ui, &["Columns as Series", "Rows as Series"], &mut i));
                    m.rows_are_series = i == 1;
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Done").clicked() {
                            finish = Some(true);
                        }
                        if ui.button("Cancel").clicked() {
                            finish = Some(false);
                        }
                    });
                });
                ui.label(RichText::new("Select the cells to plot, then click Done.").small().color(theme::TEXT_DIM()));
            });
        });
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && app.canvas.edit.is_none() {
        finish = Some(false);
    }
    app.data_refs = Some(m);
    match finish {
        Some(true) => done(app),
        Some(false) => {
            app.data_refs = None;
            select_chart(app, m.chart);
        }
        None => {}
    }
}
