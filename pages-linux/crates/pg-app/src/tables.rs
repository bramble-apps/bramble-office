//! Tables in a document (the TST engine Keydeck and Values use), with Keydeck's table chrome:
//! a selected table shows column letter / row number tabs (click to select, drag across to
//! select more, drag selected rows to move them, chevron or right-click for the row / column
//! menu, drag the lines between tabs to resize), a round corner button (click: the whole table,
//! drag: move it), "||" / "=" buttons adding columns / rows (click: a count stepper, drag: add
//! or take away), its borders drag to resize a row / column, and the selected cells show a
//! yellow autofill dot. Click a selected table to select a cell, drag across cells to select a
//! range; arrows / Tab move, typing or Return edits the cell in place (Return enters it,
//! Shift+Return is a new line in the cell, a leading "=" makes a formula), Delete clears.

use egui::{vec2, Color32, Event, Key, Pos2, Rect, Sense, Stroke, StrokeKind};
use kn_ui::theme;
use pg_core::kn_format::ObjId;
use pg_core::kn_model::tablecells::CellInput;
use pg_core::kn_model::tablerows::Axis;
use pg_core::kn_model::TableData;

use crate::app::{App, Sel};

#[derive(Default)]
struct TableUi {
    /// The cell being edited: (table, row, col, text, first frame).
    cell: Option<(ObjId, u32, u32, String, bool)>,
    /// A drag that began on a row / column tab: (table, rows?, first, last, moving rows?).
    tab_drag: Option<(ObjId, bool, u32, u32, bool)>,
    /// A border / tab line being dragged: (table, rows?, index, size before, pointer start).
    border_drag: Option<(ObjId, bool, u32, f32, Pos2)>,
    /// The open row / column tab menu: (table, rows?, index, where).
    tab_menu: Option<(ObjId, bool, u32, Pos2)>,
    /// The open add-rows / add-columns stepper: (table, rows?, where).
    add_stepper: Option<(ObjId, bool, Pos2)>,
    /// The autofill dot being dragged: (table, down? else right).
    fill_drag: Option<(ObjId, bool)>,
    /// Cells being selected by dragging: the table.
    cell_drag: Option<ObjId>,
    /// Where a run of Tab presses started: (table, row, column).
    tab_start: Option<(ObjId, u32, u32)>,
    /// The editor's wrapped line count last frame: when it changes the cell's text goes into
    /// the document so its row grows (or shrinks back) while typing, as in Pages.
    edit_rows: usize,
    /// The cell's text before live writes (Escape puts it back).
    edit_orig: Option<String>,
    popup_opened: u64,
}

thread_local! {
    static UI: std::cell::RefCell<TableUi> = std::cell::RefCell::new(TableUi::default());
}

fn with<R>(f: impl FnOnce(&mut TableUi) -> R) -> R {
    UI.with(|u| f(&mut u.borrow_mut()))
}

/// The cell under page point `pt` of table `id`.
pub fn cell_at(app: &App, id: ObjId, pt: egui::Pos2) -> Option<(u32, u32)> {
    // (a table continued over pages: the part under the pointer, its row the table's own)
    let (d, t) = app.table_at(id, pt)?;
    let col = index_at(&t.col_widths, pt.x - d.geom.x, false)?;
    let row = index_at(&t.row_heights, pt.y - d.geom.y, false)?;
    let row = t.row_map.get(row as usize).copied().unwrap_or(row);
    Some((row, col))
}

/// The row / column a distance `v` along the sizes falls in (clamped to the ends with `clamp`).
fn index_at(sizes: &[f32], v: f32, clamp: bool) -> Option<u32> {
    if sizes.is_empty() {
        return None;
    }
    if v < 0.0 {
        return clamp.then_some(0);
    }
    let mut acc = 0.0;
    for (i, s) in sizes.iter().enumerate() {
        acc += s;
        if v < acc {
            return Some(i as u32);
        }
    }
    clamp.then_some(sizes.len() as u32 - 1)
}

fn select_range(app: &mut App, id: ObjId, anchor: (u32, u32), cursor: (u32, u32)) {
    app.sel = Some(Sel { table: id, anchor, cursor });
    app.cursor = None;
    app.canvas.obj = Some(id);
    app.canvas.objs = vec![id];
    app.insp_tab = crate::app::InspTab::Cell;
}

pub fn select_cell_at(app: &mut App, id: ObjId, pt: egui::Pos2) {
    commit(app);
    if let Some((r, c)) = cell_at(app, id, pt) {
        let (r, c) = app.table(id).and_then(|(_, t)| t.merge_at(r, c).map(|m| (m.row, m.col))).unwrap_or((r, c));
        select_range(app, id, (r, c), (r, c));
        with(|u| u.tab_start = None);
    }
}

/// A press on a selected table: select that cell and follow the pointer to select a range.
pub fn press_cell(app: &mut App, id: ObjId, pt: egui::Pos2) {
    select_cell_at(app, id, pt);
    with(|u| u.cell_drag = Some(id));
}

/// Double-click: edit the cell under `pt`.
pub fn edit_cell_at(app: &mut App, id: ObjId, pt: egui::Pos2) {
    select_cell_at(app, id, pt);
    if let Some(s) = app.sel {
        open_editor(app, s.table, s.cursor.0, s.cursor.1, None);
    }
}

/// Shift-click: the selected cells reach to the cell under `pt`.
pub fn extend_to(app: &mut App, id: ObjId, pt: egui::Pos2) {
    commit(app);
    if let (Some((r, c)), Some(s)) = (cell_at(app, id, pt), app.sel) {
        app.sel = Some(Sel { cursor: (r, c), ..s });
    }
}

/// A cell's box inside the table (merged cells cover their span).
fn cell_box(t: &TableData, r: u32, c: u32) -> (f32, f32, f32, f32) {
    let (r1, c1) = t.merge_at(r, c).map(|m| (m.row + m.rows - 1, m.col + m.cols - 1)).unwrap_or((r, c));
    let x = t.col_widths.iter().take(c as usize).sum::<f32>();
    let y = t.row_heights.iter().take(r as usize).sum::<f32>();
    let w: f32 = t.col_widths.iter().skip(c as usize).take((c1 - c + 1) as usize).sum();
    let h: f32 = t.row_heights.iter().skip(r as usize).take((r1 - r + 1) as usize).sum();
    (x, y, w, h)
}

fn cell_text(t: &TableData, r: u32, c: u32) -> String {
    t.cells.get((r * t.cols + c) as usize).cloned().unwrap_or_default()
}

fn open_editor(app: &mut App, id: ObjId, r: u32, c: u32, typed: Option<String>) {
    let text = typed.unwrap_or_else(|| {
        let cur = app.table(id).map(|(_, t)| cell_text(t, r, c)).unwrap_or_default();
        let f = app.doc().and_then(|d| d.formula_text(id, r, c));
        f.map(|f| if f.starts_with('=') { f } else { format!("={f}") }).unwrap_or(cur)
    });
    with(|u| u.cell = Some((id, r, c, text, true)));
}

/// Write the cell being edited.
pub fn commit(app: &mut App) {
    let Some((t, r, c, text, _)) = with(|u| u.cell.take()) else { return };
    let old = app.table(t).map(|(_, td)| cell_text(td, r, c)).unwrap_or_default();
    let formula = app.doc().and_then(|d| d.formula_text(t, r, c));
    let same_formula = formula.as_ref().is_some_and(|f| text.trim_start().starts_with('=') && f.trim_start_matches('=') == text.trim().trim_start_matches('='));
    if (text == old && formula.is_none()) || same_formula {
        return;
    }
    let res = app.edit("Typing", |d, h| {
        if text.trim_start().starts_with('=') && text.trim().len() > 1 {
            d.set_formula_text(h, t, r, c, text.trim())
        } else {
            let r2 = d.set_cell(h, t, r, c, &CellInput::parse(&text));
            d.recalc_formulas(h, t);
            r2
        }
    });
    if let Some(Err(e)) = res {
        app.notify(format!("Couldn't change the cell: {e}"));
    }
}

pub fn editing() -> bool {
    with(|u| u.cell.is_some())
}

/// The selected cells as tab-separated text.
pub fn copy_cells(app: &App) -> Option<String> {
    let s = app.sel?;
    let (r0, c0, r1, c1) = s.block();
    let (_, t) = app.table(s.table)?;
    let tsv: Vec<String> = (r0..=r1).map(|r| (c0..=c1).map(|c| cell_text(t, r, c).replace('\n', " ")).collect::<Vec<_>>().join("\t")).collect();
    Some(tsv.join("\n"))
}

pub fn clear_cells(app: &mut App, label: &str) {
    let Some(s) = app.sel else { return };
    let (r0, c0, r1, c1) = s.block();
    let t = s.table;
    app.edit(label, |d, h| {
        for r in r0..=r1 {
            for c in c0..=c1 {
                let _ = d.set_cell(h, t, r, c, &CellInput::Empty);
            }
        }
        d.recalc_formulas(h, t);
    });
}

pub fn paste_cells(app: &mut App, text: &str) {
    let Some(s) = app.sel else { return };
    let t = s.table;
    let (r0, c0) = (s.block().0, s.block().1);
    let (rows, cols) = app.table(t).map(|(_, td)| (td.rows, td.cols)).unwrap_or((0, 0));
    let text = text.to_string();
    app.edit("Paste", |d, h| {
        for (i, line) in text.lines().enumerate() {
            for (j, v) in line.split('\t').enumerate() {
                let (r, c) = (r0 + i as u32, c0 + j as u32);
                if r < rows && c < cols {
                    let _ = d.set_cell(h, t, r, c, &CellInput::parse(v));
                }
            }
        }
        d.recalc_formulas(h, t);
    });
}

fn move_sel(app: &mut App, dr: i32, dc: i32, extend: bool) {
    let Some(s) = app.sel else { return };
    let Some((_, t)) = app.table(s.table) else { return };
    let (rows, cols) = (t.rows as i32, t.cols as i32);
    let (r, c) = s.cursor;
    let nr = (r as i32 + dr).clamp(0, rows - 1) as u32;
    let nc = (c as i32 + dc).clamp(0, cols - 1) as u32;
    app.sel = Some(if extend { Sel { cursor: (nr, nc), ..s } } else { Sel::cell(s.table, nr, nc) });
    with(|u| u.tab_start = None);
}

/// Tab / Shift-Tab: right / left, and from the last column to the next row at the column where
/// tabbing started (Keynote / Pages).
fn tab_move(app: &mut App, back: bool) {
    let Some(s) = app.sel else { return };
    let Some((_, t)) = app.table(s.table) else { return };
    let (rows, cols) = (t.rows, t.cols);
    let (r, c) = s.cursor;
    let start = with(|u| *u.tab_start.get_or_insert((s.table, r, c)));
    let (nr, nc) = if back {
        if c > 0 {
            (r, c - 1)
        } else if r > 0 {
            (r - 1, cols - 1)
        } else {
            (r, c)
        }
    } else if c + 1 < cols {
        (r, c + 1)
    } else if r + 1 < rows {
        (r + 1, if start.0 == s.table { start.2 } else { 0 })
    } else {
        (r, c)
    };
    app.sel = Some(Sel::cell(s.table, nr, nc));
}

/// Keys while cells are selected (and no cell is being edited).
pub fn handle_keys(app: &mut App, ctx: &egui::Context, tab: Option<bool>) {
    let Some(s) = app.sel else { return };
    if editing() {
        return;
    }
    if let Some(shift) = tab {
        tab_move(app, shift);
        return;
    }
    let events = ctx.input(|i| i.events.clone());
    for e in events {
        match e {
            Event::Text(t) if !t.is_empty() && !ctx.input(|i| i.modifiers.command) => {
                // typing replaces the cell's content
                let (r, c) = s.cursor;
                app.sel = Some(Sel::cell(s.table, r, c));
                open_editor(app, s.table, r, c, Some(t));
                return;
            }
            Event::Key { key, pressed: true, modifiers, .. } => match key {
                Key::Enter => {
                    let (r, c) = s.cursor;
                    open_editor(app, s.table, r, c, None);
                    return;
                }
                Key::Escape => app.sel = None,
                Key::Backspace | Key::Delete => clear_cells(app, "Delete"),
                Key::ArrowUp => move_sel(app, -1, 0, modifiers.shift),
                Key::ArrowDown => move_sel(app, 1, 0, modifiers.shift),
                Key::ArrowLeft => move_sel(app, 0, -1, modifiers.shift),
                Key::ArrowRight => move_sel(app, 0, 1, modifiers.shift),
                _ => {}
            },
            Event::Copy => {
                if let Some(tsv) = copy_cells(app) {
                    ctx.copy_text(tsv);
                }
            }
            Event::Cut => {
                if let Some(tsv) = copy_cells(app) {
                    ctx.copy_text(tsv);
                    clear_cells(app, "Cut");
                }
            }
            Event::Paste(text) => paste_cells(app, &text),
            _ => {}
        }
    }
}

/// Letters of a column (0 -> "A", 26 -> "AA").
fn column_letters(c: u32) -> String {
    let mut s = String::new();
    let mut n = c + 1;
    while n > 0 {
        s.insert(0, char::from(b'A' + ((n - 1) % 26) as u8));
        n = (n - 1) / 26;
    }
    s
}

/// Where a selected table sits on screen.
struct View {
    id: ObjId,
    page: usize,
    /// Screen point of the table's top-left.
    origin: Pos2,
    zoom: f32,
    t: TableData,
    locked: bool,
}

impl View {
    fn at(&self, x: f32, y: f32) -> Pos2 {
        self.origin + vec2(x, y) * self.zoom
    }
    fn size(&self) -> (f32, f32) {
        (self.t.col_widths.iter().sum(), self.t.row_heights.iter().sum())
    }
    fn block(&self, r0: u32, c0: u32, r1: u32, c1: u32) -> Rect {
        let (x0, y0, _, _) = cell_box(&self.t, r0, c0);
        let (x1, y1, w1, h1) = cell_box(&self.t, r1, c1);
        Rect::from_two_pos(self.at(x0, y0), self.at(x1 + w1, y1 + h1))
    }
    /// The table point under a screen point.
    fn local(&self, q: Pos2) -> egui::Vec2 {
        (q - self.origin) / self.zoom
    }
}

fn view(app: &App, id: ObjId) -> Option<View> {
    let l = app.layout.as_ref()?;
    let page = *app.obj_page.get(&id)?;
    let (d, t) = app.table(id)?;
    if d.geom.angle.rem_euclid(360.0) != 0.0 {
        return None;
    }
    Some(View { id, page, origin: app.canvas.page_to_screen(l, page, egui::pos2(d.geom.x, d.geom.y)), zoom: app.canvas.zoom, t: t.clone(), locked: d.locked })
}

/// The selected table's chrome, cell selection and cell editor (after the pages are drawn,
/// before the canvas takes the pointer). True while the pointer is the table's: the canvas
/// leaves the press alone then.
pub fn ui(app: &mut App, ui: &mut egui::Ui) -> bool {
    let table = match app.selected_objs()[..] {
        [id] if app.table(id).is_some() && app.canvas.editing.is_none() => Some(id),
        _ => None,
    };
    let Some(id) = table else {
        commit(app);
        with(|u| {
            u.tab_menu = None;
            u.add_stepper = None;
            u.tab_drag = None;
            u.border_drag = None;
            u.fill_drag = None;
            u.cell_drag = None;
        });
        return false;
    };
    if app.sel.is_some_and(|s| s.table != id) {
        app.sel = None;
    }
    if with(|u| u.cell.as_ref().is_some_and(|c| c.0 != id)) {
        commit(app);
    }
    if app.canvas.dragging() {
        return false;
    }
    let Some(v) = view(app, id) else { return false };
    let pointer = ui.input(|i| i.pointer.hover_pos());
    let mut hot = Vec::new();
    // cells being selected by dragging
    if with(|u| u.cell_drag) == Some(id) {
        if ui.input(|i| i.pointer.primary_down()) {
            if let (Some(q), Some(s)) = (pointer, app.sel) {
                let lp = v.local(q);
                if let (Some(c), Some(r)) = (index_at(&v.t.col_widths, lp.x, true), index_at(&v.t.row_heights, lp.y, true)) {
                    app.sel = Some(Sel { cursor: (r, c), ..s });
                }
            }
            ui.ctx().request_repaint();
        } else {
            with(|u| u.cell_drag = None);
        }
    }
    if !editing() {
        cell_selection(app, ui, &v, &mut hot);
        borders(app, ui, &v, &mut hot);
    }
    chrome(app, ui, &v, &mut hot);
    if let Some(r) = cell_editor(app, ui, &v) {
        hot.push(r);
    }
    let busy = with(|u| u.tab_drag.is_some() || u.border_drag.is_some() || u.fill_drag.is_some());
    busy || pointer.is_some_and(|q| hot.iter().any(|r| r.contains(q)))
}

const BAR: f32 = 20.0;
const GAP: f32 = 6.0;
const BTN: f32 = 24.0;

/// The row / column tabs, the corner button and the add buttons (Keydeck's, R32).
fn chrome(app: &mut App, ui: &mut egui::Ui, v: &View, hot: &mut Vec<Rect>) {
    let id = v.id;
    let t = &v.t;
    let (w, h) = v.size();
    let (tl, br) = (v.at(0.0, 0.0), v.at(w, h));
    let col_bar = Rect::from_min_max(egui::pos2(tl.x, tl.y - GAP - BAR), egui::pos2(br.x, tl.y - GAP));
    let row_bar = Rect::from_min_max(egui::pos2(tl.x - GAP - BAR, tl.y), egui::pos2(tl.x - GAP, br.y));
    hot.push(col_bar);
    // the row bar and its chevrons just right of it
    hot.push(Rect::from_min_max(row_bar.min, egui::pos2(row_bar.max.x + 18.0, row_bar.max.y)));
    let sel = app.sel.filter(|s| s.table == id).map(|s| s.block());
    let whole_rows = sel.is_some_and(|(_, c0, _, c1)| c0 == 0 && c1 + 1 >= t.cols && t.cols > 1);
    let whole_cols = sel.is_some_and(|(r0, _, r1, _)| r0 == 0 && r1 + 1 >= t.rows && t.rows > 1);
    let bar_fill = theme::tint(Color32::from_gray(0xf4));
    let bar_line = theme::tint(Color32::from_gray(0xc8));
    let label = theme::tint(Color32::from_gray(0x70));
    let tint = Color32::from_rgb(0xcf, 0xe0, 0xfb);
    let blue = theme::ACCENT();
    let p = ui.painter().clone();
    p.rect(col_bar, 4.0, bar_fill, Stroke::new(1.0, bar_line), StrokeKind::Inside);
    p.rect(row_bar, 4.0, bar_fill, Stroke::new(1.0, bar_line), StrokeKind::Inside);
    let font = egui::FontId::proportional(11.5);
    let mut menu_req: Option<(bool, u32, Pos2)> = None;
    let mut select: Option<(bool, u32, bool)> = None;
    let (mut col_tabs, mut row_tabs): (Vec<Rect>, Vec<Rect>) = (Vec::new(), Vec::new());
    let mut x = 0.0;
    for (c, cw) in t.col_widths.iter().enumerate() {
        let c = c as u32;
        let r = Rect::from_min_max(egui::pos2(v.at(x, 0.0).x, col_bar.min.y), egui::pos2(v.at(x + cw, 0.0).x, col_bar.max.y));
        x += cw;
        let covered = sel.is_some_and(|(_, c0, _, c1)| (c0..=c1).contains(&c));
        let solid = covered && whole_cols;
        col_tabs.push(r);
        let resp = ui.interact(r, ui.scope_id().with(("col-tab", id, c)), Sense::click_and_drag());
        if resp.drag_started() {
            with(|u| u.tab_drag = Some((id, false, c, c, false)));
        }
        let fill = if solid { blue } else if covered { tint } else if resp.hovered() { Color32::WHITE } else { Color32::TRANSPARENT };
        if fill != Color32::TRANSPARENT {
            p.rect_filled(r.shrink(1.0), 3.0, fill);
        }
        if c > 0 {
            p.vline(r.min.x, r.y_range().shrink(3.0), Stroke::new(1.0, bar_line));
        }
        theme::centered(&p, r.center(), column_letters(c), font.clone(), if solid { Color32::WHITE } else { label });
        let chevron = Rect::from_min_max(egui::pos2(r.max.x - 20.0, r.min.y), r.max);
        if resp.hovered() {
            kn_ui::icons::draw(&p, Rect::from_center_size(chevron.center(), vec2(11.0, 11.0)), kn_ui::icons::Icon::ChevronDown, theme::tint(Color32::from_gray(0x60)));
        }
        let on_chevron = resp.interact_pointer_pos().is_some_and(|q| chevron.contains(q));
        if resp.secondary_clicked() || (resp.clicked() && on_chevron) {
            menu_req = Some((false, c, egui::pos2(r.min.x, r.max.y + 2.0)));
        } else if resp.clicked() {
            select = Some((false, c, ui.input(|i| i.modifiers.shift)));
        }
    }
    let mut y = 0.0;
    for (rr, rh) in t.row_heights.iter().enumerate() {
        let ri = rr as u32;
        let r = Rect::from_min_max(egui::pos2(row_bar.min.x, v.at(0.0, y).y), egui::pos2(row_bar.max.x, v.at(0.0, y + rh).y));
        y += rh;
        let covered = sel.is_some_and(|(r0, _, r1, _)| (r0..=r1).contains(&ri));
        let solid = covered && whole_rows;
        row_tabs.push(r);
        let resp = ui.interact(r, ui.scope_id().with(("row-tab", id, ri)), Sense::click_and_drag());
        if resp.drag_started() {
            // a selected row's tab moves the selected rows; any other tab selects across
            with(|u| {
                u.tab_drag = Some(match sel {
                    Some((r0, _, r1, _)) if solid => (id, true, r0, r1, true),
                    _ => (id, true, ri, ri, false),
                })
            });
        }
        let fill = if solid { blue } else if covered { tint } else if resp.hovered() { Color32::WHITE } else { Color32::TRANSPARENT };
        if fill != Color32::TRANSPARENT {
            p.rect_filled(r.shrink(1.0), 3.0, fill);
        }
        if ri > 0 {
            p.hline(r.x_range().shrink(3.0), r.min.y, Stroke::new(1.0, bar_line));
        }
        theme::centered(&p, r.center(), format!("{}", ri + 1), font.clone(), if solid { Color32::WHITE } else { label });
        let chev = Rect::from_center_size(egui::pos2(r.max.x + 10.0, r.center().y), vec2(16.0, r.height().min(22.0)));
        let cresp = ui.interact(chev, ui.scope_id().with(("row-chev", id, ri)), Sense::click());
        if resp.hovered() || cresp.hovered() {
            p.rect_filled(chev, 3.0, Color32::WHITE);
            kn_ui::icons::draw(&p, Rect::from_center_size(chev.center(), vec2(11.0, 11.0)), kn_ui::icons::Icon::ChevronDown, theme::tint(Color32::from_gray(0x60)));
        }
        if resp.secondary_clicked() || cresp.clicked() {
            menu_req = Some((true, ri, egui::pos2(r.max.x + 2.0, r.min.y)));
        } else if resp.clicked() {
            select = Some((true, ri, ui.input(|i| i.modifiers.shift)));
        }
    }
    // the lines between tabs: drag one to resize the row / column before it
    for (rows, tabs) in [(false, &col_tabs), (true, &row_tabs)] {
        for (i, r) in tabs.iter().enumerate() {
            let zone = if rows { Rect::from_x_y_ranges(r.x_range(), (r.max.y - 4.0)..=(r.max.y + 4.0)) } else { Rect::from_x_y_ranges((r.max.x - 4.0)..=(r.max.x + 4.0), r.y_range()) };
            let resp = ui.interact(zone, ui.scope_id().with(("tab-line", id, rows, i)), Sense::drag());
            if resp.hovered() || resp.dragged() {
                ui.ctx().set_cursor_icon(if rows { egui::CursorIcon::ResizeRow } else { egui::CursorIcon::ResizeColumn });
            }
            if resp.drag_started() {
                let size = if rows { t.row_heights[i] } else { t.col_widths[i] };
                if let Some(q) = ui.input(|inp| inp.pointer.press_origin()) {
                    with(|u| u.border_drag = Some((id, rows, i as u32, size, q)));
                }
            }
        }
    }
    // a drag that began on a tab: select across tabs, or move the selected rows
    if let Some((tid, rows, lo, hi, moving)) = with(|u| u.tab_drag).filter(|d| d.0 == id) {
        let tabs = if rows { &row_tabs } else { &col_tabs };
        let q = ui.input(|i| i.pointer.latest_pos());
        let along = |q: Pos2| if rows { q.y } else { q.x };
        if let (Some(q), false) = (q, tabs.is_empty()) {
            let vv = along(q);
            let under = tabs.iter().position(|r| vv < if rows { r.max.y } else { r.max.x }).unwrap_or(tabs.len() - 1) as u32;
            if moving {
                let (first, end) = (t.header_rows, t.rows.saturating_sub(t.footer_rows));
                let edges: Vec<f32> = std::iter::once(tabs[0].min.y).chain(tabs.iter().map(|r| r.max.y)).collect();
                let before = (first..=end).min_by(|a, b| (edges[*a as usize] - vv).abs().total_cmp(&(edges[*b as usize] - vv).abs())).unwrap_or(first);
                p.hline(row_bar.min.x..=br.x, edges[before as usize], Stroke::new(3.0, blue));
                let carried = Rect::from_min_max(egui::pos2(row_bar.min.x, tabs[lo as usize].min.y), egui::pos2(br.x, tabs[hi as usize].max.y));
                p.rect_filled(carried.translate(vec2(0.0, vv - carried.center().y)), 2.0, blue.gamma_multiply(0.18));
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                if ui.input(|i| i.pointer.any_released()) {
                    with(|u| u.tab_drag = None);
                    if !(lo..=hi + 1).contains(&before) {
                        match app.edit(if hi > lo { "Move Rows" } else { "Move Row" }, |d, h| d.move_table_rows(h, tid, lo, hi, before)) {
                            Some(Ok(at)) => select_range(app, tid, (at, 0), (at + hi - lo, t.cols - 1)),
                            Some(Err(e)) => app.notify(e.to_string()),
                            None => {}
                        }
                    }
                }
            } else {
                let (a, b) = (lo.min(under), lo.max(under));
                if rows {
                    select_range(app, tid, (a, 0), (b, t.cols - 1));
                } else {
                    select_range(app, tid, (0, a), (t.rows - 1, b));
                }
                if ui.input(|i| i.pointer.any_released()) {
                    with(|u| u.tab_drag = None);
                }
            }
            ui.ctx().request_repaint();
        }
        if !ui.input(|i| i.pointer.any_down()) {
            with(|u| u.tab_drag = None);
        }
    }
    if let Some((rows, i, shift)) = select {
        commit(app);
        let anchor = app.sel.filter(|s| s.table == id && shift).map(|s| if rows { s.anchor.0 } else { s.anchor.1 }).unwrap_or(i);
        let (a, b) = (anchor.min(i), anchor.max(i));
        if rows {
            select_range(app, id, (a, 0), (b, t.cols - 1));
        } else {
            select_range(app, id, (0, a), (t.rows - 1, b));
        }
    }
    if let Some((rows, i, pos)) = menu_req {
        commit(app);
        if rows {
            select_range(app, id, (i, 0), (i, t.cols - 1));
        } else {
            select_range(app, id, (0, i), (t.rows - 1, i));
        }
        let pass = ui.ctx().cumulative_pass_nr();
        with(|u| {
            u.tab_menu = Some((id, rows, i, pos));
            u.popup_opened = pass;
        });
    }
    // corner button: the whole table (click) / move it (drag); "||" and "=": add columns / rows
    let round = |p: &egui::Painter, c: Pos2, hover: bool, glyph: &str| {
        p.circle(c, BTN / 2.0, if hover { blue } else { Color32::WHITE }, Stroke::new(1.0, if hover { blue } else { bar_line }));
        let ink = Stroke::new(1.6, if hover { Color32::WHITE } else { label });
        match glyph {
            "o" => {
                p.circle_stroke(c, 4.8, ink);
            }
            "||" => {
                p.vline(c.x - 2.3, (c.y - 5.0)..=(c.y + 5.0), ink);
                p.vline(c.x + 2.3, (c.y - 5.0)..=(c.y + 5.0), ink);
            }
            _ => {
                p.hline((c.x - 5.0)..=(c.x + 5.0), c.y - 2.3, ink);
                p.hline((c.x - 5.0)..=(c.x + 5.0), c.y + 2.3, ink);
            }
        }
    };
    let corner = egui::pos2(row_bar.center().x, col_bar.center().y);
    let corner_r = Rect::from_center_size(corner, vec2(BTN, BTN));
    hot.push(corner_r);
    let rc = ui.interact(corner_r, ui.scope_id().with(("tbl-all", id)), Sense::click_and_drag());
    round(&p, corner, rc.hovered() || rc.dragged(), "o");
    if rc.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    if rc.clicked() {
        commit(app);
        app.sel = None;
    }
    if rc.drag_started() && !v.locked {
        commit(app);
        app.sel = None;
        if let Some(q) = ui.input(|i| i.pointer.press_origin()) {
            crate::canvas::start_move(app, id, v.page, q);
        }
    }
    for (rows, c, glyph) in [(false, egui::pos2(col_bar.max.x + GAP + BTN / 2.0, col_bar.center().y), "||"), (true, egui::pos2(row_bar.center().x, row_bar.max.y + GAP + BTN / 2.0), "=")] {
        let btn = Rect::from_center_size(c, vec2(BTN, BTN));
        hot.push(btn);
        let resp = ui.interact(btn, ui.scope_id().with(("tbl-add", id, rows)), Sense::click_and_drag());
        round(&p, c, resp.hovered() || resp.dragged(), glyph);
        if resp.hovered() || resp.dragged() {
            ui.ctx().set_cursor_icon(if rows { egui::CursorIcon::ResizeVertical } else { egui::CursorIcon::ResizeHorizontal });
        }
        if resp.clicked() {
            let pass = ui.ctx().cumulative_pass_nr();
            with(|u| {
                u.add_stepper = if u.add_stepper.is_some_and(|s| s.0 == id && s.1 == rows) { None } else { Some((id, rows, c + vec2(BTN, 0.0))) };
                u.popup_opened = pass;
            });
        }
        let (cur, last) = if rows { (t.rows, t.row_heights.last().copied().unwrap_or(30.0)) } else { (t.cols, t.col_widths.last().copied().unwrap_or(80.0)) };
        let step = (last * v.zoom).max(8.0);
        // dragging back only takes away empty rows / columns at the end
        let empty = |k: u32| {
            let cell = |r: u32, c: u32| t.cells.get((r * t.cols + c) as usize).is_none_or(|x| x.trim().is_empty());
            if rows { (0..t.cols).all(|c| cell(k, c)) } else { (0..t.rows).all(|r| cell(r, k)) }
        };
        let floor = (1..cur).rev().take_while(|&k| empty(k)).last().unwrap_or(cur).max(1);
        let origin_id = ui.scope_id().with(("tbl-add-origin", id, rows));
        if resp.drag_started() {
            if let Some(o) = ui.input(|i| i.pointer.press_origin()) {
                ui.data_mut(|d| d.insert_temp(origin_id, o));
            }
        }
        let origin: Option<Pos2> = ui.data(|d| d.get_temp(origin_id));
        let target = |q: Pos2| {
            let d = origin.map(|o| if rows { q.y - o.y } else { q.x - o.x }).unwrap_or(0.0);
            (cur as f32 + (d / step).round()).clamp(floor as f32, 255.0) as u32
        };
        if resp.dragged() {
            if let Some(q) = resp.interact_pointer_pos() {
                let n = target(q);
                let edge = if rows { br.y + (n as f32 - cur as f32) * step } else { br.x + (n as f32 - cur as f32) * step };
                let line = if rows { [egui::pos2(tl.x, edge), egui::pos2(br.x, edge)] } else { [egui::pos2(edge, tl.y), egui::pos2(edge, br.y)] };
                p.line_segment(line, Stroke::new(2.0, blue));
                let what = if rows { if n == 1 { "row" } else { "rows" } } else if n == 1 { "column" } else { "columns" };
                tip(&p, q, format!("{n} {what}"));
            }
            ui.ctx().request_repaint();
        }
        if resp.drag_stopped() {
            if let Some(q) = resp.interact_pointer_pos().or(ui.input(|i| i.pointer.latest_pos())) {
                let n = target(q);
                if n != cur {
                    set_count(app, id, rows, cur, n);
                }
            }
        }
    }
    tab_menu(app, ui, hot);
    add_stepper(app, ui, t.rows, t.cols, hot);
}

fn tip(p: &egui::Painter, q: Pos2, text: String) {
    let g = p.layout_no_wrap(text, egui::FontId::proportional(12.0), Color32::WHITE);
    let at = q + vec2(14.0, 14.0);
    p.rect_filled(Rect::from_min_size(at, g.size() + vec2(12.0, 6.0)), 4.0, Color32::from_black_alpha(200));
    p.galley(at + vec2(6.0, 3.0), g, Color32::WHITE);
}

/// A row / column tab's menu (rows have no Sort items, as in Pages / Keynote).
fn tab_menu(app: &mut App, ui: &mut egui::Ui, hot: &mut Vec<Rect>) {
    let Some((id, is_row, i, pos)) = with(|u| u.tab_menu) else { return };
    let mut close = false;
    let r = egui::Area::new(ui.scope_id().with(("tab-menu", id))).order(egui::Order::Foreground).fixed_pos(pos).show(ui.ctx(), |ui| {
        egui::Frame::menu(ui.style()).show(ui, |ui| {
            ui.set_min_width(210.0);
            ui.style_mut().visuals.button_frame = false;
            ui.style_mut().spacing.button_padding = vec2(8.0, 3.0);
            let item = |ui: &mut egui::Ui, app: &mut App, label: &str, cmd: &str| -> bool {
                let on = crate::menus::enabled(app, cmd);
                let clicked = ui.add_enabled(on, egui::Button::new(label).min_size(vec2(210.0, 0.0))).clicked();
                if clicked {
                    let ctx = ui.ctx().clone();
                    crate::menus::run(app, &ctx, cmd);
                }
                clicked
            };
            if is_row {
                close |= item(ui, app, "Add Row Above", "Table > Add Row Above");
                close |= item(ui, app, "Add Row Below", "Table > Add Row Below");
                ui.separator();
                close |= item(ui, app, "Delete Row", "Table > Delete Row");
            } else {
                close |= item(ui, app, "Sort Ascending", "Table > Sort Ascending");
                close |= item(ui, app, "Sort Descending", "Table > Sort Descending");
                ui.separator();
                close |= item(ui, app, "Add Column Before", "Table > Add Column Before");
                close |= item(ui, app, "Add Column After", "Table > Add Column After");
                ui.separator();
                close |= item(ui, app, "Delete Column", "Table > Delete Column");
            }
            ui.separator();
            let fit = if is_row { "Fit Height to Content" } else { "Fit Width to Content" };
            if ui.add(egui::Button::new(fit).min_size(vec2(210.0, 0.0))).clicked() {
                app.edit(fit, |d, h| d.fit_table_to_content(h, id, is_row, &[i]));
                close = true;
            }
            ui.separator();
            for l in ["Cut", "Copy", "Paste"] {
                close |= item(ui, app, l, l);
            }
            if ui.add(egui::Button::new("Clear All").min_size(vec2(210.0, 0.0))).clicked() {
                clear_cells(app, "Clear All");
                close = true;
            }
        });
    });
    hot.push(r.response.rect);
    let just_opened = ui.ctx().cumulative_pass_nr() <= with(|u| u.popup_opened) + 1;
    if close || (r.response.clicked_elsewhere() && !just_opened) || ui.input(|i| i.key_pressed(Key::Escape)) {
        with(|u| u.tab_menu = None);
    }
}

/// The add-rows / add-columns count stepper.
fn add_stepper(app: &mut App, ui: &mut egui::Ui, rows: u32, cols: u32, hot: &mut Vec<Rect>) {
    let Some((id, is_rows, pos)) = with(|u| u.add_stepper) else { return };
    let cur = if is_rows { rows } else { cols };
    let mut want = cur;
    let r = egui::Area::new(ui.scope_id().with(("add-stepper", id))).order(egui::Order::Foreground).fixed_pos(pos).show(ui.ctx(), |ui| {
        egui::Frame::menu(ui.style()).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(if is_rows { "Rows" } else { "Columns" });
                ui.add(egui::DragValue::new(&mut want).range(1..=255));
                if ui.small_button("▲").clicked() {
                    want = cur + 1;
                }
                if ui.small_button("▼").clicked() && cur > 1 {
                    want = cur - 1;
                }
            });
        });
    });
    hot.push(r.response.rect);
    if want != cur {
        set_count(app, id, is_rows, cur, want);
    }
    let just_opened = ui.ctx().cumulative_pass_nr() <= with(|u| u.popup_opened) + 1;
    if (r.response.clicked_elsewhere() && !just_opened) || ui.input(|i| i.key_pressed(Key::Escape)) {
        with(|u| u.add_stepper = None);
    }
}

/// Give a table `want` rows / columns (from `cur`): new ones copy the last one's size, and the
/// last ones go first.
fn set_count(app: &mut App, id: ObjId, is_rows: bool, cur: u32, want: u32) {
    commit(app);
    let axis = if is_rows { Axis::Row } else { Axis::Column };
    let res = app.edit(if is_rows { "Rows" } else { "Columns" }, |d, h| {
        let mut n = cur;
        let mut res = Ok(());
        while res.is_ok() && n < want {
            res = d.table_insert(h, id, axis, n, n - 1);
            n += 1;
        }
        while res.is_ok() && n > want.max(1) {
            res = d.table_delete(h, id, axis, n - 1);
            n -= 1;
        }
        res
    });
    if let Some(Err(e)) = res {
        app.notify(e.to_string());
    }
    if let Some(s) = app.sel {
        let (rows, cols) = (if is_rows { want } else { app.table(id).map(|(_, t)| t.rows).unwrap_or(1) }, if is_rows { app.table(id).map(|(_, t)| t.cols).unwrap_or(1) } else { want });
        let fit = |p: (u32, u32)| (p.0.min(rows.max(1) - 1), p.1.min(cols.max(1) - 1));
        app.sel = Some(Sel { anchor: fit(s.anchor), cursor: fit(s.cursor), ..s });
    }
}

/// The table's column and row borders: drag one to resize that column / row (the table grows
/// or shrinks with it). The table's own handles win over the borders under them.
fn borders(app: &mut App, ui: &mut egui::Ui, v: &View, hot: &mut Vec<Rect>) {
    let id = v.id;
    let t = &v.t;
    let (w, h) = v.size();
    let mut lines: Vec<(bool, u32, Rect)> = Vec::new();
    let mut y = 0.0;
    for (i, rh) in t.row_heights.iter().enumerate() {
        y += rh;
        let (a, b) = (v.at(0.0, y), v.at(w, y));
        lines.push((true, i as u32, Rect::from_min_max(egui::pos2(a.x, a.y - 3.0), egui::pos2(b.x, b.y + 3.0))));
    }
    let mut x = 0.0;
    for (i, cw) in t.col_widths.iter().enumerate() {
        x += cw;
        let (a, b) = (v.at(x, 0.0), v.at(x, h));
        lines.push((false, i as u32, Rect::from_min_max(egui::pos2(a.x - 3.0, a.y), egui::pos2(b.x + 3.0, b.y))));
    }
    // the selection handles (shown while no cells are selected) cut out of the lines
    if app.sel.is_none() {
        let (a, b) = (v.at(0.0, 0.0), v.at(w, h));
        let handles: Vec<Rect> = [a.x, (a.x + b.x) / 2.0, b.x].iter().flat_map(|&x| [a.y, (a.y + b.y) / 2.0, b.y].map(|y| Rect::from_center_size(egui::pos2(x, y), vec2(14.0, 14.0)))).collect();
        lines = lines
            .into_iter()
            .flat_map(|(rows, i, r)| {
                let mut pieces = vec![r];
                for hr in handles.iter().filter(|hr| hr.intersects(r)) {
                    pieces = pieces
                        .into_iter()
                        .flat_map(|p| {
                            if !hr.intersects(p) {
                                return vec![p];
                            }
                            if rows {
                                vec![Rect::from_x_y_ranges(p.min.x..=hr.min.x, p.y_range()), Rect::from_x_y_ranges(hr.max.x..=p.max.x, p.y_range())]
                            } else {
                                vec![Rect::from_x_y_ranges(p.x_range(), p.min.y..=hr.min.y), Rect::from_x_y_ranges(p.x_range(), hr.max.y..=p.max.y)]
                            }
                        })
                        .filter(|p| p.width() > 1.0 && p.height() > 1.0)
                        .collect();
                }
                pieces.into_iter().map(move |p| (rows, i, p))
            })
            .collect();
    }
    let pointer = ui.input(|i| i.pointer.interact_pos());
    for (k, (rows, i, r)) in lines.into_iter().enumerate() {
        let resp = ui.interact(r, ui.scope_id().with(("table-border", id, rows, i, k)), Sense::drag());
        if resp.hovered() || resp.dragged() {
            ui.ctx().set_cursor_icon(if rows { egui::CursorIcon::ResizeRow } else { egui::CursorIcon::ResizeColumn });
            hot.push(r);
        }
        if resp.drag_started() {
            let size = if rows { t.row_heights[i as usize] } else { t.col_widths[i as usize] };
            if let Some(p) = ui.input(|inp| inp.pointer.press_origin()).or(pointer) {
                with(|u| u.border_drag = Some((id, rows, i, size, p)));
            }
        }
    }
    let Some((bid, rows, i, size, start)) = with(|u| u.border_drag) else { return };
    if bid != id {
        with(|u| u.border_drag = None);
        return;
    }
    let Some(p) = pointer.or(ui.input(|i| i.pointer.latest_pos())) else { return };
    let delta = if rows { p.y - start.y } else { p.x - start.x } / v.zoom;
    let new = (size + delta).max(if rows { 12.0 } else { 20.0 });
    let before: f32 = if rows { t.row_heights[..i as usize].iter().sum() } else { t.col_widths[..i as usize].iter().sum() };
    let painter = ui.painter();
    let (a, b) = if rows { (v.at(0.0, before + new), v.at(w, before + new)) } else { (v.at(before + new, 0.0), v.at(before + new, h)) };
    painter.line_segment([a, b], Stroke::new(2.0, theme::ACCENT()));
    tip(painter, p, format!("{} {:.0} pt", if rows { "Height" } else { "Width" }, new));
    ui.ctx().request_repaint();
    if !ui.input(|i| i.pointer.primary_down()) {
        with(|u| u.border_drag = None);
        if (new - size).abs() > 0.5 {
            app.edit(if rows { "Resize Row" } else { "Resize Column" }, |d, h| d.set_table_sizes(h, id, rows, &[(i, new)]));
        }
    }
}

/// The selected cells: a thin blue outline with round handles at two corners, and a yellow dot
/// at the middle of the bottom / right edge when pointed at; dragging the dot fills the cells it
/// covers (autofill).
fn cell_selection(app: &mut App, ui: &mut egui::Ui, v: &View, hot: &mut Vec<Rect>) {
    let Some(s) = app.sel.filter(|s| s.table == v.id) else {
        with(|u| u.fill_drag = None);
        return;
    };
    let (r0, c0, r1, c1) = app.with_merges(v.id, s.block());
    let r = v.block(r0, c0, r1, c1);
    let p = ui.painter().clone();
    let blue = theme::ACCENT();
    p.rect_filled(r, 0.0, blue.gamma_multiply(0.10));
    p.rect_stroke(r, 0.0, Stroke::new(1.5, blue), StrokeKind::Middle);
    for c in [r.left_top(), r.right_bottom()] {
        p.circle(c, 3.5, Color32::WHITE, Stroke::new(1.2, blue));
    }
    let yellow = Color32::from_rgb(0xf5, 0xc2, 0x11);
    let pointer = ui.input(|i| i.pointer.hover_pos());
    let dragging_cells = with(|u| u.cell_drag.is_some());
    for (down, dot) in [(true, r.center_bottom()), (false, r.right_center())] {
        let hit = Rect::from_center_size(dot, vec2(12.0, 12.0));
        let resp = ui.interact(hit, ui.scope_id().with(("autofill", v.id, down)), Sense::drag());
        let show = !dragging_cells && (with(|u| u.fill_drag.is_some_and(|f| f.1 == down)) || resp.hovered() || pointer.is_some_and(|q| (q - dot).length() < 14.0));
        if show {
            p.circle(dot, 4.5, yellow, Stroke::new(1.0, Color32::WHITE));
            ui.ctx().set_cursor_icon(if down { egui::CursorIcon::ResizeVertical } else { egui::CursorIcon::ResizeHorizontal });
            hot.push(hit);
        }
        if resp.drag_started() {
            with(|u| u.fill_drag = Some((v.id, down)));
        }
    }
    let Some((fid, down)) = with(|u| u.fill_drag) else { return };
    if fid != v.id {
        with(|u| u.fill_drag = None);
        return;
    }
    let Some(q) = ui.input(|i| i.pointer.latest_pos()) else { return };
    let lp = v.local(q);
    let (to_r1, to_c1) = if down { (index_at(&v.t.row_heights, lp.y, true).unwrap_or(r1).max(r1), c1) } else { (r1, index_at(&v.t.col_widths, lp.x, true).unwrap_or(c1).max(c1)) };
    p.rect_stroke(v.block(r0, c0, to_r1, to_c1), 0.0, Stroke::new(3.0, yellow), StrokeKind::Middle);
    ui.ctx().request_repaint();
    if !ui.input(|i| i.pointer.primary_down()) {
        with(|u| u.fill_drag = None);
        if (to_r1, to_c1) != (r1, c1) {
            let tid = v.id;
            if let Some(Err(e)) = app.edit("Autofill", |d, h| d.autofill(h, tid, (r0, c0, r1, c1), (r0, c0, to_r1, to_c1))) {
                app.notify(e.to_string());
            }
            app.sel = Some(Sel { table: tid, anchor: (r0, c0), cursor: (to_r1, to_c1) });
        }
    }
}

/// The cell references in a formula, in order: ((row, col), (row, col)) blocks.
fn formula_refs(text: &str) -> Vec<((u32, u32), (u32, u32))> {
    let parse = |w: &str| -> Option<(u32, u32)> {
        let w = w.replace('$', "");
        let split = w.find(|c: char| c.is_ascii_digit())?;
        let (l, d) = w.split_at(split);
        if l.is_empty() || l.len() > 3 || !l.chars().all(|c| c.is_ascii_alphabetic()) || !d.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        let col = l.to_ascii_uppercase().bytes().fold(0u32, |a, c| a * 26 + u32::from(c - b'A' + 1)) - 1;
        Some((d.parse::<u32>().ok()?.checked_sub(1)?, col))
    };
    let mut words: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut in_str = false;
    for ch in text.chars() {
        if ch == '"' {
            in_str = !in_str;
        }
        if !in_str && (ch.is_ascii_alphanumeric() || ch == '$' || ch == ':') {
            word.push(ch);
        } else if !word.is_empty() {
            words.push(std::mem::take(&mut word));
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
        .iter()
        .filter_map(|w| match w.split_once(':') {
            Some((a, b)) => Some((parse(a)?, parse(b)?)),
            None => parse(w).map(|a| (a, a)),
        })
        .collect()
}

/// Take a key press with exactly these modifiers (Shift or not, nothing else) out of the input.
fn take_key(i: &mut egui::InputState, key: Key, shift: bool) -> bool {
    let want = egui::Modifiers { shift, ..egui::Modifiers::NONE };
    let before = i.events.len();
    i.events.retain(|e| !matches!(e, Event::Key { key: k, pressed: true, modifiers, .. } if *k == key && *modifiers == want));
    i.events.len() != before
}

/// The cell editor, in the cell's own size, colour and alignment (a formula gets Keydeck's
/// formula bar). Return enters the cell, Shift+Return starts a new line in it, Tab moves on,
/// Escape cancels. Returns the editor's screen rect.
fn cell_editor(app: &mut App, ui: &mut egui::Ui, v: &View) -> Option<Rect> {
    let (id, r, c, mut text, first) = with(|u| u.cell.take())?;
    if id != v.id || r >= v.t.rows || c >= v.t.cols {
        return None;
    }
    let (x, y, w, h) = cell_box(&v.t, r, c);
    let cell = Rect::from_min_size(v.at(x, y), vec2(w, h) * v.zoom);
    let formula = text.trim_start().starts_with('=');
    let style = v.t.cell_styles.get((r * v.t.cols + c) as usize).cloned().unwrap_or_default();
    let c32 = |k: pg_core::kn_model::Color| Color32::from_rgba_unmultiplied((k.r * 255.0) as u8, (k.g * 255.0) as u8, (k.b * 255.0) as u8, (k.a * 255.0) as u8);
    let font_px = if formula { 14.0 } else { (if style.text.size > 0.0 { style.text.size } else { 11.0 }) * v.zoom };
    let line_h = font_px * 1.25;
    let lines = text.split('\n').count().max(1) as f32;
    let edit_id = egui::Id::unique(("cell-editor", id));
    let focused = ui.memory(|m| m.has_focus(edit_id));
    // Return enters the cell, Tab moves on; Shift+Return is left to the editor (a new line)
    let (enter, tab, back_tab) = if focused {
        // (egui's consume_key ignores Shift, so the modifiers are matched exactly here)
        ui.input_mut(|i| (take_key(i, Key::Enter, false), take_key(i, Key::Tab, false), take_key(i, Key::Tab, true)))
    } else {
        (false, false, false)
    };
    let esc = ui.input(|i| i.key_pressed(Key::Escape));
    let mut accept = false;
    let mut cancel = false;
    let (rect, area) = if formula {
        let colors = [Color32::from_rgb(0x35, 0x84, 0xe4), Color32::from_rgb(0xff, 0x78, 0x00), Color32::from_rgb(0x91, 0x41, 0xac), Color32::from_rgb(0xe0, 0x1b, 0x24), Color32::from_rgb(0x26, 0xa2, 0x69)];
        for (k, (a, b)) in formula_refs(&text).into_iter().enumerate() {
            if a.0 < v.t.rows && b.0 < v.t.rows && a.1 < v.t.cols && b.1 < v.t.cols {
                let col = colors[k % colors.len()];
                ui.painter().rect(v.block(a.0, a.1, b.0, b.1), 2.0, col.gamma_multiply(0.15), Stroke::new(1.5, col), StrokeKind::Middle);
            }
        }
        let bar_h = 28.0;
        let bar = Rect::from_min_size(egui::pos2(cell.min.x - 1.0, cell.center().y - bar_h / 2.0), vec2(cell.width().max(280.0), bar_h));
        let p = ui.painter();
        p.rect_filled(bar.translate(vec2(0.0, 2.0)).expand(1.0), 8.0, Color32::from_black_alpha(40));
        p.rect(bar, 7.0, Color32::WHITE, Stroke::new(1.0, theme::tint(Color32::from_gray(0xb8))), StrokeKind::Outside);
        let fx = Rect::from_min_size(bar.min, vec2(30.0, bar_h));
        p.rect_filled(fx, egui::CornerRadius { nw: 7, sw: 7, ne: 0, se: 0 }, theme::tint(Color32::from_gray(0xf2)));
        p.vline(fx.max.x, bar.y_range(), Stroke::new(1.0, theme::tint(Color32::from_gray(0xd8))));
        theme::centered(p, fx.center(), "ƒx", egui::FontId::proportional(13.0), theme::tint(Color32::from_gray(0x60)));
        let ok = Rect::from_center_size(bar.right_center() - vec2(15.0, 0.0), vec2(20.0, 20.0));
        let no = Rect::from_center_size(bar.right_center() - vec2(39.0, 0.0), vec2(20.0, 20.0));
        p.circle_filled(ok.center(), 9.0, Color32::from_rgb(0x2e, 0xc2, 0x7e));
        theme::centered(p, ok.center(), "✓", egui::FontId::proportional(12.0), Color32::WHITE);
        p.circle_filled(no.center(), 9.0, Color32::from_rgb(0xe0, 0x1b, 0x24));
        theme::centered(p, no.center(), "✕", egui::FontId::proportional(11.0), Color32::WHITE);
        accept = ui.interact(ok, ui.scope_id().with("formula-ok"), Sense::click()).clicked();
        cancel = ui.interact(no, ui.scope_id().with("formula-cancel"), Sense::click()).clicked();
        (Rect::from_min_max(egui::pos2(fx.max.x + 6.0, bar.center().y - 9.0), egui::pos2(no.min.x - 6.0, bar.center().y + 9.0)), bar)
    } else {
        // in place, over the cell's own background; it grows down for the lines it needs
        let back = match &style.fill {
            pg_core::kn_model::Fill::Color(k) if k.a > 0.99 => c32(*k),
            pg_core::kn_model::Fill::Gradient { stops, .. } if !stops.is_empty() => c32(stops[0].1).to_opaque(),
            _ => Color32::WHITE,
        };
        let [pl, pt, pr, pb] = style.padding.map(|x| x * v.zoom);
        let need = lines * line_h + pt + pb;
        let area = Rect::from_min_size(cell.min, vec2(cell.width(), cell.height().max(need)));
        ui.painter().rect_filled(area, 0.0, back);
        let inner = Rect::from_min_max(area.min + vec2(pl, pt), area.max - vec2(pr, pb));
        let text_h = lines * line_h;
        let top = match style.valign {
            pg_core::kn_model::VAlign::Top => inner.min.y,
            pg_core::kn_model::VAlign::Middle => inner.center().y - text_h / 2.0,
            pg_core::kn_model::VAlign::Bottom => inner.max.y - text_h,
        };
        (Rect::from_min_size(egui::pos2(inner.min.x, top.max(inner.min.y)), vec2(inner.width().max(10.0), text_h)), area)
    };
    let halign = match style.align {
        _ if formula => egui::Align::Min,
        pg_core::kn_model::Align::Center => egui::Align::Center,
        pg_core::kn_model::Align::Right => egui::Align::Max,
        pg_core::kn_model::Align::Natural if text.trim().parse::<f64>().is_ok() => egui::Align::Max,
        _ => egui::Align::Min,
    };
    let color = if formula || style.text.color.a <= 0.0 { Color32::BLACK } else { c32(style.text.color) };
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    let edit = egui::TextEdit::multiline(&mut text)
        .id(edit_id)
        .font(egui::FontId::proportional(font_px))
        .text_color(color)
        .align(egui::Align2([halign, egui::Align::Min]))
        .margin(egui::Margin::ZERO)
        .frame(egui::Frame::NONE)
        .desired_rows(1)
        .desired_width(rect.width());
    let out = edit.show(&mut child);
    let resp = out.response.clone();
    let rows = out.galley.rows.len().max(1);
    if !formula {
        ui.painter().rect_stroke(area, 0.0, Stroke::new(2.0, theme::ACCENT()), StrokeKind::Outside);
    }
    if first {
        with(|u| {
            u.edit_rows = rows;
            u.edit_orig = None;
        });
        resp.request_focus();
        // the caret after what is there (a fresh editor would otherwise start with no caret)
        let mut st = egui::text_edit::TextEditState::load(ui.ctx(), edit_id).unwrap_or_default();
        let n = text.chars().count();
        st.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(n))));
        st.store(ui.ctx(), edit_id);
        with(|u| u.cell = Some((id, r, c, text, false)));
        return Some(area);
    }
    if esc || cancel {
        // the text written while typing goes back to what the cell held
        if let Some(orig) = with(|u| u.edit_orig.take()) {
            app.edit("Typing", |d, h| {
                let r2 = d.set_cell(h, id, r, c, &CellInput::parse(&orig));
                d.recalc_formulas(h, id);
                r2
            });
        }
        return Some(area);
    }
    if resp.lost_focus() || enter || tab || back_tab || accept {
        with(|u| u.cell = Some((id, r, c, text, false)));
        commit(app);
        app.sel = Some(Sel::cell(id, r, c));
        if tab || back_tab {
            tab_move(app, back_tab);
        }
        return Some(area);
    }
    // the row grows with the text while typing (Pages): the cell is written when the editor's
    // line count changes; Return still enters it as one step
    if !formula && with(|u| std::mem::replace(&mut u.edit_rows, rows)) != rows && text.contains('\n') | (rows > 1) {
        let tid = id;
        // (a trailing new line counts as a line: a space holds it until Return writes the text)
        let value = if text.ends_with('\n') { format!("{text} ") } else { text.clone() };
        let orig = app.table(tid).map(|(_, td)| cell_text(td, r, c)).unwrap_or_default();
        with(|u| {
            u.edit_orig.get_or_insert(orig);
        });
        app.edit("Typing", |d, h| {
            let r2 = d.set_cell(h, tid, r, c, &CellInput::parse(&value));
            d.recalc_formulas(h, tid);
            r2
        });
    }
    with(|u| u.cell = Some((id, r, c, text, false)));
    Some(area)
}

/// Table > Select ▸, Autofill Cells ▸, Resize Row / Column to Fit Content, Formula > New Formula,
/// Text > Auto-Align Table Cell, Repeat Header Rows on Each Page (Keydeck's tableops, on
/// Letters' cell selection). False when `name` isn't one of them.
pub fn run_command(app: &mut App, name: &str) -> bool {
    let table = app.sel.map(|s| s.table).or_else(|| app.selected_objs().into_iter().find(|id| app.table(*id).is_some()));
    let Some(id) = table else { return false };
    let Some(t) = app.table(id).map(|(_, t)| t.clone()) else { return false };
    let s = app.sel.filter(|s| s.table == id).unwrap_or(Sel { table: id, anchor: (0, 0), cursor: (0, 0) });
    let (r0, c0, r1, c1) = s.block();
    let (cr, cc) = s.cursor;
    let last_row = t.rows.saturating_sub(1);
    let last_col = t.cols.saturating_sub(1);
    let filled_in_row = |r: u32| (0..t.cols).filter(|&c| !cell_text(&t, r, c).trim().is_empty()).collect::<Vec<u32>>();
    let filled_in_col = |c: u32| (0..t.rows).filter(|&r| !cell_text(&t, r, c).trim().is_empty()).collect::<Vec<u32>>();
    commit(app);
    match name {
        "Select > Row" => select_range(app, id, (cr, 0), (cr, last_col)),
        "Select > Column" => select_range(app, id, (0, cc), (last_row, cc)),
        "Select > First Value in Row" | "Select > Last Value in Row" | "Select > Extend to First Value in Row" | "Select > Extend to Last Value in Row" => {
            let f = filled_in_row(cr);
            let target = if name.contains("First") { f.first().copied().unwrap_or(0) } else { f.last().copied().unwrap_or(last_col) };
            if name.contains("Extend") {
                select_range(app, id, (cr, cc), (cr, target));
            } else {
                select_range(app, id, (cr, target), (cr, target));
            }
        }
        "Select > First Value in Column" | "Select > Last Value in Column" | "Select > Extend to First Value in Column" | "Select > Extend to Last Value in Column" => {
            let f = filled_in_col(cc);
            let target = if name.contains("First") { f.first().copied().unwrap_or(0) } else { f.last().copied().unwrap_or(last_row) };
            if name.contains("Extend") {
                select_range(app, id, (cr, cc), (target, cc));
            } else {
                select_range(app, id, (target, cc), (target, cc));
            }
        }
        "Table > Resize Row to Fit Content" | "Table > Resize Column to Fit Content" => {
            let rows = name.contains("Row");
            let idx: Vec<u32> = if rows { (r0..=r1).collect() } else { (c0..=c1).collect() };
            app.edit(if rows { "Fit Height to Content" } else { "Fit Width to Content" }, |d, h| d.fit_table_to_content(h, id, rows, &idx));
        }
        "Table > Repeat Header Rows on Each Page" => {
            let model = app.doc().and_then(|d| d.file.obj(id)).and_then(|m| m.ref_id("tableModel"));
            if let Some(m) = model {
                let on = !app.doc().and_then(|d| d.file.obj(m)).and_then(|mm| mm.bool("repeating_header_rows_enabled")).unwrap_or(false);
                app.edit("Repeat Header Rows", |d, h| {
                    d.touch(h, m);
                    if let Some(mm) = d.file.obj_mut(m) {
                        mm.set_bool("repeating_header_rows_enabled", on);
                    }
                });
            }
        }
        "Formula > New Formula" => {
            select_range(app, id, (cr, cc), (cr, cc));
            open_editor(app, id, cr, cc, Some("=".into()));
        }
        "Text > Auto-Align Table Cell" => {
            let blocks = app.model_blocks(id, (r0, c0, r1, c1));
            app.edit("Auto-Align", |d, h| {
                for b in &blocks {
                    let _ = d.set_cells_text_prop(h, id, *b, &pg_core::kn_model::format::TextProp::Align(pg_core::kn_model::Align::Natural));
                }
            });
        }
        _ => {
            let n = name.trim_start_matches("Autofill Cells > ");
            if !name.starts_with("Autofill Cells > ") {
                return false;
            }
            let mut edits: Vec<(u32, u32, String)> = Vec::new();
            match n {
                "Autofill from Row Above" if cr > 0 => edits.push((cr, cc, cell_text(&t, cr - 1, cc))),
                "Autofill from Column Before" if cc > 0 => edits.push((cr, cc, cell_text(&t, cr, cc - 1))),
                "Autofill Right" | "Autofill Left" | "Autofill Down" | "Autofill Up" | "Autofill" => {
                    // the first cell(s) on the side the fill starts from set the pattern: two
                    // numbers continue their step, anything else is copied
                    let (along_rows, reverse) = match n {
                        "Autofill Down" => (true, false),
                        "Autofill" => (r1 > r0, false),
                        "Autofill Up" => (true, true),
                        "Autofill Left" => (false, true),
                        _ => (false, false),
                    };
                    let lines: Vec<u32> = if along_rows { (c0..=c1).collect() } else { (r0..=r1).collect() };
                    for line in lines {
                        let mut idx: Vec<u32> = if along_rows { (r0..=r1).collect() } else { (c0..=c1).collect() };
                        if reverse {
                            idx.reverse();
                        }
                        let get = |k: u32| if along_rows { cell_text(&t, k, line) } else { cell_text(&t, line, k) };
                        let first = get(idx[0]);
                        let step = (idx.len() > 2).then(|| (first.trim().parse::<f64>().ok(), get(idx[1]).trim().parse::<f64>().ok())).and_then(|(a, b)| Some((a?, b? - a?)));
                        for (k_i, &k) in idx.iter().enumerate().skip(if step.is_some() { 2 } else { 1 }) {
                            let v = match step {
                                Some((a, d)) => {
                                    let x = a + d * k_i as f64;
                                    if x.fract() == 0.0 { format!("{x:.0}") } else { format!("{x}") }
                                }
                                None => first.clone(),
                            };
                            edits.push(if along_rows { (k, line, v) } else { (line, k, v) });
                        }
                    }
                }
                _ => return true,
            }
            if !edits.is_empty() {
                app.edit(n, |d, h| {
                    for (r, c, v) in &edits {
                        let _ = d.set_cell(h, id, *r, *c, &CellInput::parse(v));
                    }
                    d.recalc_formulas(h, id);
                });
            }
        }
    }
    true
}

/// The table commands `run_command` handles.
pub fn is_command(name: &str) -> bool {
    name.starts_with("Select > ")
        || name.starts_with("Autofill Cells > ")
        || matches!(name, "Table > Resize Row to Fit Content" | "Table > Resize Column to Fit Content" | "Table > Repeat Header Rows on Each Page" | "Formula > New Formula" | "Text > Auto-Align Table Cell")
}
