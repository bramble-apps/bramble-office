//! Edit Chart Data: the suite's chart data editor (Keydeck, Values and Letters). It follows
//! the chart's type and how it reads its grid (kn_model::chartdata::ChartLayout): Plot Rows /
//! Plot Columns as Series, series colour swatches, X / Y / Size labels on scatter and bubble
//! series (shared or separate X), and pies' unused categories dimmed. The window only edits
//! its copy; the app applies each `DataAction` as one undo step.

use egui::{Color32, RichText};
use kn_format::ObjId;
use kn_model::chartdata::{ChartGrid, ChartLayout, SeriesRole};
use kn_model::{ChartData, Document};

use crate::theme;

pub struct ChartDataEditor {
    pub chart: ObjId,
    names_r: Vec<String>,
    names_c: Vec<String>,
    cells: Vec<Vec<String>>,
    layout: ChartLayout,
    /// The plotted series' colours (in plot order).
    colors: Vec<Color32>,
}

/// A change to apply (one undo step each).
#[derive(Debug, Clone, PartialEq)]
pub enum DataAction {
    None,
    /// The grid: Document::set_chart_grid ("Edit Chart Data").
    Grid(ChartGrid),
    /// Plot Rows as Series (true) / Plot Columns as Series: Document::set_chart_series_direction.
    Direction(bool),
    /// Scatter / bubble Share X Values: Document::set_chart_shared_x.
    SharedX(bool),
}

impl DataAction {
    pub fn undo_name(&self) -> &'static str {
        match self {
            DataAction::Direction(true) => "Plot Rows as Series",
            DataAction::Direction(false) => "Plot Columns as Series",
            DataAction::SharedX(_) => "Share X Values",
            _ => "Edit Chart Data",
        }
    }

    /// Apply to the document (inside the app's undo step).
    pub fn apply(&self, doc: &mut Document, h: &mut kn_model::edit::History, chart: ObjId) -> bool {
        match self {
            DataAction::None => false,
            DataAction::Grid(g) => doc.set_chart_grid(h, chart, g),
            DataAction::Direction(by_row) => doc.set_chart_series_direction(h, chart, *by_row),
            DataAction::SharedX(s) => doc.set_chart_shared_x(h, chart, *s),
        }
    }
}

fn fmt(v: f64) -> String {
    if v.is_nan() {
        String::new()
    } else if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

fn c32(c: kn_model::Color) -> Color32 {
    Color32::from_rgba_unmultiplied((c.r * 255.0) as u8, (c.g * 255.0) as u8, (c.b * 255.0) as u8, (c.a * 255.0) as u8)
}

impl ChartDataEditor {
    /// The editor for `chart`; `data` (the drawable's resolved chart) gives the series colours.
    pub fn open(doc: &Document, chart: ObjId, data: Option<&ChartData>) -> Option<ChartDataEditor> {
        let g = doc.chart_grid(chart)?;
        let layout = doc.chart_layout(chart)?;
        Some(ChartDataEditor {
            chart,
            names_r: g.row_names.clone(),
            names_c: g.col_names.clone(),
            cells: g.values.iter().map(|r| r.iter().map(|v| fmt(*v)).collect()).collect(),
            layout,
            colors: data.map(|c| c.series.iter().map(|s| c32(s.color)).collect()).unwrap_or_default(),
        })
    }

    /// After an action was applied: the chart's type / direction / colours as they are now.
    pub fn refresh(&mut self, doc: &Document, data: Option<&ChartData>) {
        if let Some(l) = doc.chart_layout(self.chart) {
            self.layout = l;
        }
        if let Some(c) = data {
            self.colors = c.series.iter().map(|s| c32(s.color)).collect();
        }
    }

    fn grid(&self) -> ChartGrid {
        ChartGrid {
            row_names: self.names_r.clone(),
            col_names: self.names_c.clone(),
            values: self.cells.iter().map(|r| r.iter().map(|s| s.trim().replace(',', "").parse::<f64>().unwrap_or(f64::NAN)).collect()).collect(),
        }
    }

    fn series_count(&self) -> usize {
        if self.layout.by_row { self.names_r.len() } else { self.names_c.len() }
    }

    /// Series `i`'s colour: its plotted series (scatter / bubble groups share their Y's).
    fn color(&self, i: usize) -> Option<Color32> {
        if !self.layout.is_xy() {
            return self.colors.get(i).copied();
        }
        let ys = (0..=i).filter(|k| self.layout.role(*k) == SeriesRole::Y).count();
        let k = match self.layout.role(i) {
            SeriesRole::X if self.layout.shared_x => return None,
            // separate X: the X before its Y
            SeriesRole::X => ys,
            _ => ys.checked_sub(1)?,
        };
        self.colors.get(k).copied()
    }

    fn add_series(&mut self) {
        let n = self.series_count() + 1;
        if self.layout.by_row {
            self.names_r.push(format!("Series {n}"));
            self.cells.push(vec![String::new(); self.names_c.len()]);
        } else {
            self.names_c.push(format!("Series {n}"));
            self.cells.iter_mut().for_each(|r| r.push(String::new()));
        }
    }

    fn add_category(&mut self) {
        if self.layout.by_row {
            self.names_c.push(format!("Category {}", self.names_c.len() + 1));
            self.cells.iter_mut().for_each(|r| r.push(String::new()));
        } else {
            self.names_r.push(format!("Category {}", self.names_r.len() + 1));
            self.cells.push(vec![String::new(); self.names_c.len()]);
        }
    }

    fn remove_row(&mut self) {
        self.names_r.pop();
        self.cells.pop();
    }

    fn remove_col(&mut self) {
        self.names_c.pop();
        self.cells.iter_mut().for_each(|r| {
            r.pop();
        });
    }

    /// The window. Returns whether it's still open and what to apply.
    pub fn window(&mut self, ctx: &egui::Context) -> (bool, DataAction) {
        let mut open = true;
        let mut act = DataAction::None;
        let mut grid_changed = false;
        egui::Window::new("Chart Data").pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).collapsible(false).resizable(true).default_width(560.0).open(&mut open).show(ctx, |ui| {
            let lay = self.layout.clone();
            // Plot Rows as Series | Plot Columns as Series
            let mut dir = usize::from(!lay.by_row);
            ui.allocate_ui(egui::vec2(ui.available_width().min(380.0), 28.0), |ui| switch(ui, &["Plot Rows as Series", "Plot Columns as Series"], &mut dir));
            if dir != usize::from(!lay.by_row) {
                act = DataAction::Direction(dir == 0);
            }
            if lay.is_xy() {
                let mut shared = lay.shared_x;
                if theme::checkbox(ui, &mut shared, "Share X Values").changed() {
                    act = DataAction::SharedX(shared);
                }
            }
            ui.horizontal(|ui| {
                if ui.button("+ Series").clicked() {
                    self.add_series();
                    grid_changed = true;
                }
                if ui.button("+ Category").clicked() {
                    self.add_category();
                    grid_changed = true;
                }
                let (series_rows, cat_rows) = if lay.by_row { (self.names_r.len(), self.names_c.len()) } else { (self.names_c.len(), self.names_r.len()) };
                if ui.add_enabled(series_rows > 1, egui::Button::new("− Series")).clicked() {
                    if lay.by_row { self.remove_row() } else { self.remove_col() }
                    grid_changed = true;
                }
                if ui.add_enabled(cat_rows > 1, egui::Button::new("− Category")).clicked() {
                    if lay.by_row { self.remove_col() } else { self.remove_row() }
                    grid_changed = true;
                }
            });
            ui.separator();
            let role_tag = |r: SeriesRole| match r {
                SeriesRole::X => Some("X"),
                SeriesRole::Y => Some("Y"),
                SeriesRole::Size => Some("Size"),
                SeriesRole::Values => None,
            };
            let colors: Vec<Option<Color32>> = (0..self.series_count()).map(|i| self.color(i)).collect();
            egui::ScrollArea::both().max_height(380.0).show(ui, |ui| {
                egui::Grid::new("chart-data-grid").striped(true).spacing([4.0, 4.0]).show(ui, |ui| {
                    // series by column: a row of X / Y / Size tags above the names
                    if !lay.by_row && lay.is_xy() {
                        ui.label("");
                        for j in 0..self.names_c.len() {
                            ui.label(RichText::new(role_tag(lay.role(j)).unwrap_or("")).small().strong().color(theme::ACCENT()));
                        }
                        ui.end_row();
                    }
                    ui.label("");
                    for (j, c) in self.names_c.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            if !lay.by_row {
                                swatch(ui, colors.get(j).copied().flatten());
                            }
                            let dim = lay.by_row && !lay.category_used(j);
                            grid_changed |= ui.add_sized([if lay.by_row { 96.0 } else { 80.0 }, 22.0], egui::TextEdit::singleline(c).font(egui::FontId::proportional(13.0)).text_color_opt(dim.then(theme::TEXT_DIM))).lost_focus();
                        });
                    }
                    ui.end_row();
                    for (r, name) in self.names_r.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            if lay.by_row {
                                swatch(ui, colors.get(r).copied().flatten());
                                if let Some(t) = role_tag(lay.role(r)) {
                                    ui.label(RichText::new(t).small().strong().color(theme::ACCENT()));
                                }
                            }
                            let dim = !lay.by_row && !lay.category_used(r);
                            grid_changed |= ui.add_sized([110.0, 22.0], egui::TextEdit::singleline(name).font(egui::FontId::proportional(13.0)).text_color_opt(dim.then(theme::TEXT_DIM))).lost_focus();
                        });
                        for (j, cell) in self.cells[r].iter_mut().enumerate() {
                            let used = if lay.by_row { lay.category_used(j) } else { lay.category_used(r) };
                            let resp = ui.add_sized([96.0, 22.0], egui::TextEdit::singleline(cell).text_color_opt((!used).then(theme::TEXT_DIM)));
                            grid_changed |= resp.lost_focus();
                        }
                        ui.end_row();
                    }
                });
            });
            let (series, cats) = if lay.by_row { ("rows", "columns") } else { ("columns", "rows") };
            let note = if lay.is_round() {
                format!("Each series ({series}) is a wedge, sized by its first value; the other {cats} aren't drawn.")
            } else if lay.is_bubble() {
                format!("Series are {series}: X, Y and bubble Size{}.", if lay.shared_x { " (one X for every series)" } else { " for each series" })
            } else if lay.is_xy() {
                format!("Series are {series}: X and Y values{}.", if lay.shared_x { " (one X for every series)" } else { " for each series" })
            } else {
                format!("Series are {series}, categories are {cats}. Empty cells are left blank in the chart.")
            };
            ui.label(RichText::new(note).small().color(theme::TEXT_DIM()));
        });
        if grid_changed && act == DataAction::None {
            act = DataAction::Grid(self.grid());
        }
        (open, act)
    }
}

fn swatch(ui: &mut egui::Ui, c: Option<Color32>) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
    if let Some(c) = c {
        ui.painter().rect_filled(r, 2.0, c);
    }
}

/// The apps' inner switch (Style | Layout): a rounded segmented control.
pub(crate) fn switch(ui: &mut egui::Ui, labels: &[&str], cur: &mut usize) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 26.0), egui::Sense::hover());
    let p = ui.painter().clone();
    p.rect_filled(rect, 7.0, theme::gray(0x33));
    let w = rect.width() / labels.len() as f32;
    for (i, l) in labels.iter().enumerate() {
        let r = egui::Rect::from_min_size(egui::pos2(rect.min.x + w * i as f32, rect.min.y), egui::vec2(w, rect.height()));
        let resp = ui.interact(r, ui.scope_id().with(("chart-data-switch", i)), egui::Sense::click());
        let sel = *cur == i;
        if sel {
            p.rect_filled(r.shrink(1.0), 6.0, theme::ACCENT());
        } else if resp.hovered() {
            p.rect_filled(r.shrink(2.0), 5.0, theme::gray(0x40));
        }
        theme::centered(&p, r.center(), l, egui::FontId::proportional(12.5), if sel { Color32::WHITE } else { theme::TEXT() });
        if resp.clicked() {
            *cur = i;
        }
    }
}
