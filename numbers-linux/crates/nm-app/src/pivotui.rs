//! Organize > Create Pivot Table and the Pivot Options pane: a table summarizing another one
//! (nm_core::pivot), on a new sheet or the current one, rebuilt when its fields change and with
//! Refresh Pivot Table. Values keeps which table a pivot table summarizes while the document
//! is open; the pivot table itself is a table of the document.

use egui::RichText;
use kn_ui::theme;
use nm_core::kn_format::ObjId;
use nm_core::kn_model::tablecells::CellInput;
use nm_core::kn_model::tablerows::{Axis, HeaderKind};
use nm_core::pivot::{Cell, PivotSpec, ShowAs, Summary};

use crate::app::App;
use crate::canvas::commit;

/// A pivot table: the table it summarizes and its fields.
#[derive(Clone, Debug)]
pub struct PivotOf {
    pub source: ObjId,
    pub spec: PivotSpec,
    /// When it was last refreshed (Pivot Options: "Last refreshed").
    pub refreshed: Option<std::time::SystemTime>,
    /// Made by Numbers: its totals live in its summary model, so Values shows its own
    /// computation over the stored cells and doesn't rewrite them.
    pub native: bool,
}

pub const MENU: &[&str] = &[
    "Create Pivot Table > On New Sheet",
    "Create Pivot Table > On Current Sheet",
    "Create Pivot Table > For Selected Cells on New Sheet",
    "Create Pivot Table > For Selected Cells on Current Sheet",
    "Show Pivot Options",
    "Refresh Pivot Table",
    "Create Table for Source Data",
];

pub fn enabled(app: &App, name: &str) -> bool {
    let t = app.sel.map(|s| s.table);
    match name {
        "Show Pivot Options" | "Refresh Pivot Table" | "Create Table for Source Data" => t.is_some_and(|t| app.pivots.contains_key(&t)),
        n if n.starts_with("Create Pivot Table > For Selected") => app.sel.is_some_and(|s| s.anchor.0 != s.cursor.0) && t.is_some_and(|t| !app.pivots.contains_key(&t)),
        _ => t.is_some_and(|t| !app.pivots.contains_key(&t)),
    }
}

/// The table's data as stored (not as categories show it).
fn source_data(app: &App, t: ObjId) -> Option<nm_core::kn_model::TableData> {
    app.wb.as_ref()?.table_data(t)
}

/// Fill (or first make) the pivot table for `p`; its id.
fn write(app: &mut App, sheet: ObjId, existing: Option<ObjId>, p: &PivotOf, at: (f32, f32)) -> Option<ObjId> {
    let src = source_data(app, p.source)?;
    let pv = nm_core::pivot::build(&src, &p.spec);
    let rows = pv.cells.len() as u32;
    let cols = pv.cells.iter().map(|r| r.len()).max().unwrap_or(1) as u32;
    let donor = nm_core::sheets::table_style_donor(0).ok();
    let res = app.edit(if existing.is_some() { "Refresh Pivot Table" } else { "Create Pivot Table" }, |d, h| -> Result<ObjId, nm_core::kn_format::Error> {
        let t = match existing {
            Some(t) => t,
            None => {
                let (k, info) = donor.as_ref().ok_or(nm_core::kn_format::Error::Invalid("no table donor".into()))?;
                nm_core::sheets::insert_table(d, h, sheet, k, *info, at.0, at.1)?
            }
        };
        // the pivot's size
        let shape = |d: &nm_core::kn_model::Document| d.file.obj(t).and_then(|m| nm_core::kn_model::table::table_shape(&d.file, m)).map(|s| (s.rows, s.cols)).unwrap_or((1, 1));
        let (mut r, mut c) = shape(d);
        while r < rows {
            d.table_insert(h, t, Axis::Row, r, r - 1)?;
            r += 1;
        }
        while r > rows.max(1) {
            d.table_delete(h, t, Axis::Row, r - 1)?;
            r -= 1;
        }
        while c < cols {
            d.table_insert(h, t, Axis::Column, c, c - 1)?;
            c += 1;
        }
        while c > cols.max(1) {
            d.table_delete(h, t, Axis::Column, c - 1)?;
            c -= 1;
        }
        d.set_table_header_count(h, t, HeaderKind::Rows, pv.header_rows.min(rows))?;
        d.set_table_header_count(h, t, HeaderKind::Columns, pv.header_cols.min(cols))?;
        d.set_table_header_count(h, t, HeaderKind::Footer, 1.min(rows - pv.header_rows.min(rows)))?;
        for (ri, row) in pv.cells.iter().enumerate() {
            for ci in 0..cols as usize {
                let v = match row.get(ci) {
                    Some(Cell::Text(s)) => CellInput::Text(s.clone()),
                    Some(Cell::Number(x)) => CellInput::Number((x * 1e9).round() / 1e9),
                    _ => CellInput::Empty,
                };
                d.set_cell(h, t, ri as u32, ci as u32, &v)?;
            }
        }
        // Show as percentages: those cells as percents, the other values automatic
        if rows > pv.header_rows && cols > pv.header_cols {
            let _ = d.set_cells_format(h, t, (pv.header_rows, pv.header_cols, rows - 1, cols - 1), &nm_core::kn_model::tablefmt::CellFormat::Automatic);
        }
        for &(r, c) in &pv.percent {
            let _ = d.set_cells_format(h, t, (r, c, r, c), &nm_core::kn_model::tablefmt::CellFormat::Percent { decimals: Some(1), thousands: false, negative: 0 });
        }
        Ok(t)
    });
    match res {
        Some(Ok(t)) => Some(t),
        Some(Err(e)) => {
            app.notify(format!("Pivot table: {e}"));
            None
        }
        None => None,
    }
}

/// The default fields: the first text column as rows, the first number column summed.
fn default_spec(src: &nm_core::kn_model::TableData, only: Option<(u32, u32)>) -> PivotSpec {
    let body = src.header_rows..src.rows.saturating_sub(src.footer_rows);
    let numeric = |c: u32| body.clone().filter(|r| !src.cells.get((r * src.cols + c) as usize).map(|s| s.trim().is_empty()).unwrap_or(true)).all(|r| src.cells[(r * src.cols + c) as usize].replace([',', '$', '%'], "").trim().parse::<f64>().is_ok());
    let text_col = (0..src.cols).find(|c| !numeric(*c));
    let num_col = (0..src.cols).find(|c| numeric(*c) && body.clone().any(|r| !src.cells[(r * src.cols + c) as usize].trim().is_empty()));
    PivotSpec { rows: text_col.into_iter().collect(), values: num_col.map(|c| (c, Summary::Sum)).into_iter().collect(), only, ..Default::default() }
}

pub fn run(app: &mut App, name: &str) {
    commit(app);
    let Some(sel) = app.sel else { return };
    let t = sel.table;
    match name {
        "Show Pivot Options" => {
            app.inspector = true;
            app.insp_tab = crate::app::InspTab::Organize;
        }
        "Refresh Pivot Table" => refresh(app, t),
        "Create Table for Source Data" => {
            // the source table selected (on its sheet)
            if let Some(src) = app.pivots.get(&t).map(|p| p.source) {
                let sheets = app.sheets();
                if let Some(i) = sheets.iter().position(|s| s.drawables.contains(&src)) {
                    app.sheet = i;
                    app.canvas.reset();
                }
                app.sel = Some(crate::app::Sel::cell(src, 0, 0));
            }
        }
        n => {
            let Some(src) = source_data(app, t) else { return };
            let only = n.contains("Selected").then(|| {
                let b = sel.block();
                (app.mrow(t, b.0).unwrap_or(b.0), app.mrow(t, b.2).unwrap_or(b.2))
            });
            let p = PivotOf { source: t, spec: default_spec(&src, only), refreshed: Some(std::time::SystemTime::now()), native: false };
            if n.ends_with("New Sheet") {
                app.add_sheet_empty("Pivot Table");
            }
            let Some(sheet) = app.sheets().get(app.sheet).map(|s| s.id) else { return };
            let at = if n.ends_with("New Sheet") { (0.0, 0.0) } else { app.free_spot_pub() };
            if let Some(pt) = write(app, sheet, None, &p, at) {
                app.pivots.insert(pt, p);
                app.sel = Some(crate::app::Sel::cell(pt, 0, 0));
                app.inspector = true;
                app.insp_tab = crate::app::InspTab::Organize;
            }
        }
    }
}

pub fn refresh(app: &mut App, t: ObjId) {
    if let Some(p) = app.pivots.get_mut(&t) {
        p.refreshed = Some(std::time::SystemTime::now());
    }
    let Some(p) = app.pivots.get(&t).cloned() else { return };
    if p.native {
        // shown afresh from its source (App::refresh); its own cells stay Numbers'
        app.changed();
        return;
    }
    let Some(sheet) = app.sheets().iter().find(|s| s.drawables.contains(&t)).map(|s| s.id) else { return };
    write(app, sheet, Some(t), &p, (0.0, 0.0));
}

/// "Last refreshed" as Numbers words it: today's time, else the date.
fn when(t: std::time::SystemTime) -> String {
    let secs = t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let ago = now - secs;
    match ago {
        i64::MIN..=59 => "Just now".into(),
        60..=3599 => format!("{} min ago", ago / 60),
        3600..=86399 => format!("{} h ago", ago / 3600),
        _ => format!("{} days ago", ago / 86400),
    }
}

/// Organize > Pivot Options: Source Data (with Refresh), the Columns, Rows and Values wells
/// (add, remove, each well's … menu, Summarize by and Show as), as Numbers' pane.
pub fn options(app: &mut App, ui: &mut egui::Ui, t: ObjId) {
    let Some(mut p) = app.pivots.get(&t).cloned() else { return };
    let Some(src) = source_data(app, p.source) else { return };
    let before = p.spec.clone();
    let name = |c: u32| nm_core::pivot::field_name(&src, c);
    let mut refresh_now = false;
    ui.label(theme::bold("Source Data"));
    ui.horizontal(|ui| {
        ui.label(&src.name);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            refresh_now = ui.small_button("⟳").on_hover_text("Refresh Pivot Table").clicked();
        });
    });
    if let Some(at) = p.refreshed {
        ui.label(RichText::new(format!("Last refreshed: {}", when(at))).small().color(theme::TEXT_DIM()));
    }
    // a well: its fields, an add menu and its … menu
    let well = |ui: &mut egui::Ui, title: &str, list: &mut Vec<u32>, id: &str, menu: &mut dyn FnMut(&mut egui::Ui)| {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(theme::bold(title));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button("…", |ui| menu(ui));
            });
        });
        let mut drop = None;
        for (i, c) in list.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(name(*c));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("🗑").on_hover_text("Remove").clicked() {
                        drop = Some(i);
                    }
                });
            });
        }
        if let Some(i) = drop {
            list.remove(i);
        }
        theme::combo(id).selected_text("Add Field…").width(ui.available_width()).show_ui(ui, |ui| {
            for c in 0..src.cols {
                if ui.selectable_label(false, name(c)).clicked() {
                    list.push(c);
                }
            }
        });
    };
    let mut swap = false;
    let (mut hide_col, mut hide_row) = (p.spec.hide_total_column, p.spec.hide_total_row);
    let mut clear_cols = false;
    let mut clear_rows = false;
    well(ui, "Columns", &mut p.spec.cols, "pivot-cols", &mut |ui| {
        if ui.button(if hide_col { "Show Grand Total Column" } else { "Hide Grand Total Column" }).clicked() {
            hide_col = !hide_col;
            ui.close();
        }
        if ui.button("Swap Column and Row Fields").clicked() {
            swap = true;
            ui.close();
        }
        if ui.button("Clear Column Fields").clicked() {
            clear_cols = true;
            ui.close();
        }
    });
    well(ui, "Rows", &mut p.spec.rows, "pivot-rows", &mut |ui| {
        if ui.button(if hide_row { "Show Grand Total Row" } else { "Hide Grand Total Row" }).clicked() {
            hide_row = !hide_row;
            ui.close();
        }
        if ui.button("Swap Column and Row Fields").clicked() {
            swap = true;
            ui.close();
        }
        if ui.button("Clear Row Fields").clicked() {
            clear_rows = true;
            ui.close();
        }
    });
    (p.spec.hide_total_column, p.spec.hide_total_row) = (hide_col, hide_row);
    if swap {
        std::mem::swap(&mut p.spec.rows, &mut p.spec.cols);
    }
    if clear_cols {
        p.spec.cols.clear();
    }
    if clear_rows {
        p.spec.rows.clear();
    }
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label(theme::bold("Values"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.menu_button("…", |ui| {
                if ui.button("Clear Value Fields").clicked() {
                    p.spec.values.clear();
                    p.spec.show_as.clear();
                    ui.close();
                }
            });
        });
    });
    let mut drop = None;
    p.spec.show_as.resize(p.spec.values.len(), ShowAs::Base);
    for (i, (c, s)) in p.spec.values.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.label(format!("{} ({})", name(*c), s.name()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("🗑").on_hover_text("Remove").clicked() {
                    drop = Some(i);
                }
            });
        });
        egui::Grid::new(("pivot-val", i)).num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            ui.label(RichText::new("Summarize by").small());
            theme::combo(("pivot-sum", i)).selected_text(s.name()).width(150.0).show_ui(ui, |ui| {
                for k in Summary::ALL {
                    ui.selectable_value(s, k, k.name());
                    if k.ends_group() {
                        ui.separator();
                    }
                }
            });
            ui.end_row();
            ui.label(RichText::new("Show as").small());
            let show = &mut p.spec.show_as[i];
            theme::combo(("pivot-show", i)).selected_text(show.name()).width(150.0).show_ui(ui, |ui| {
                for k in ShowAs::ALL {
                    ui.selectable_value(show, k, k.name());
                }
            });
            ui.end_row();
        });
    }
    if let Some(i) = drop {
        p.spec.values.remove(i);
        p.spec.show_as.remove(i);
    }
    theme::combo("pivot-vals").selected_text("Add Field…").width(ui.available_width()).show_ui(ui, |ui| {
        for c in 0..src.cols {
            if ui.selectable_label(false, name(c)).clicked() {
                p.spec.values.push((c, Summary::Sum));
                p.spec.show_as.push(ShowAs::Base);
            }
        }
    });
    ui.add_space(10.0);
    refresh_now |= ui.add_sized([ui.available_width(), 28.0], egui::Button::new("Refresh Pivot Table")).clicked();
    if p.spec != before || refresh_now {
        app.pivots.insert(t, p);
        refresh(app, t);
    }
}
