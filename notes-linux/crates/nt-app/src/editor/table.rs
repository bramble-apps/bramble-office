//! Tables: rows and columns added and deleted, Tab from cell to cell (a new row after the last),
//! Convert to Text, selecting rows / columns / cells (the handles and the yellow outline Notes
//! draws), dragging a column's edge, and drawing the grid (1 pt #AAAAAA lines, as Notes' PDF).

use egui::{Color32, CornerRadius, Painter, Pos2, Rect, Stroke, pos2, vec2};

use super::doc::{Para, Table};
use super::input::Drag;
use super::layout::{self, Block, Flow, PBox, TableBox};
use super::ops::{Frag, Kind};
use super::{Cell, Editor, Loc, TableSel};

impl Editor {
    /// The table the caret or the cell selection is in: (its paragraph, its attachment id).
    fn cur_table(&self) -> Option<(usize, String)> {
        let para = self.table_sel.map(|s| s.para).or(self.head.cell.map(|c| c.para))?;
        let id = self.doc.paras.get(para)?.sole_att()?.to_string();
        self.doc.table(&id)?;
        Some((para, id))
    }

    /// The rows and columns the command applies to: the selection's, else the caret's cell.
    fn cur_span(&self) -> Option<(usize, usize, usize, usize)> {
        if let Some(s) = self.table_sel {
            return Some((s.r0, s.c0, s.r1, s.c1));
        }
        self.head.cell.map(|c| (c.row, c.col, c.row, c.col))
    }

    fn edit_table(&mut self, f: impl FnOnce(&mut Table, (usize, usize, usize, usize)) -> Option<Cell>) {
        let (Some((para, id)), Some(span)) = (self.cur_table(), self.cur_span()) else { return };
        self.push_undo(Kind::Other);
        let t = self.doc.table_mut(&id).unwrap();
        let to = f(t, span);
        self.table_sel = None;
        if let Some(c) = to {
            let l = Loc { cell: Some(Cell { para, ..c }), para: 0, off: 0 };
            self.anchor = l;
            self.head = l;
        }
        self.changed = true;
    }

    pub fn table_add_row(&mut self, below: bool) {
        self.edit_table(|t, (r0, c0, r1, _)| {
            let at = if below { r1 + 1 } else { r0 };
            t.cells.insert(at, vec![vec![Para::default()]; t.cols()]);
            Some(Cell { para: 0, row: at, col: c0 })
        });
    }

    pub fn table_add_col(&mut self, after: bool) {
        self.edit_table(|t, (r0, c0, _, c1)| {
            let at = if after { c1 + 1 } else { c0 };
            for row in &mut t.cells {
                row.insert(at, vec![Para::default()]);
            }
            t.widths.insert(at.min(t.widths.len()), None);
            Some(Cell { para: 0, row: r0, col: at })
        });
    }

    pub fn table_delete_rows(&mut self) {
        let Some((para, _)) = self.cur_table() else { return };
        let rows = self.cur_table().and_then(|(_, id)| self.doc.table(&id).map(|t| t.rows())).unwrap_or(0);
        let span = self.cur_span().unwrap_or_default();
        if span.0 == 0 && span.2 + 1 >= rows {
            self.delete_table(para);
            return;
        }
        self.edit_table(|t, (r0, c0, r1, _)| {
            t.cells.drain(r0..=r1);
            Some(Cell { para: 0, row: r0.min(t.rows() - 1), col: c0 })
        });
    }

    pub fn table_delete_cols(&mut self) {
        let Some((para, id)) = self.cur_table() else { return };
        let cols = self.doc.table(&id).map_or(0, |t| t.cols());
        let span = self.cur_span().unwrap_or_default();
        if span.1 == 0 && span.3 + 1 >= cols {
            self.delete_table(para);
            return;
        }
        self.edit_table(|t, (r0, c0, _, c1)| {
            for row in &mut t.cells {
                row.drain(c0..=c1);
            }
            if t.widths.len() > c1 {
                t.widths.drain(c0..=c1);
            }
            Some(Cell { para: 0, row: r0, col: c0.min(t.cols() - 1) })
        });
    }

    fn delete_table(&mut self, para: usize) {
        self.push_undo(Kind::Other);
        self.doc.paras[para] = Para::default();
        self.table_sel = None;
        let l = Loc { cell: None, para, off: 0 };
        self.anchor = l;
        self.head = l;
        self.changed = true;
    }

    /// Format > Convert to Text: one paragraph per row, cells separated by tabs.
    pub fn table_to_text(&mut self) {
        let Some((para, id)) = self.cur_table() else { return };
        self.push_undo(Kind::Other);
        let t = self.doc.table(&id).unwrap().clone();
        let mut paras = Vec::new();
        for row in &t.cells {
            let mut p = Para::default();
            for (c, cell) in row.iter().enumerate() {
                if c > 0 {
                    let n = p.len();
                    p.insert(n, "\t", &Default::default());
                }
                for (i, cp) in cell.iter().enumerate() {
                    if i > 0 {
                        let n = p.len();
                        p.insert(n, " ", &Default::default());
                    }
                    p.append(&cp.slice(0..cp.len()));
                }
            }
            paras.push(p);
        }
        let n = paras.len();
        self.doc.paras.splice(para..=para, paras);
        self.table_sel = None;
        let l = Loc { cell: None, para: para + n - 1, off: self.doc.paras[para + n - 1].len() };
        self.anchor = l;
        self.head = l;
        self.changed = true;
    }

    /// Tab / Shift-Tab in a cell: the next / previous cell (Tab in the last adds a row).
    pub fn table_tab(&mut self, back: bool) {
        let Some(c) = self.head.cell else { return };
        let Some((_, id)) = self.cur_table() else { return };
        let (rows, cols) = self.doc.table(&id).map(|t| (t.rows(), t.cols())).unwrap();
        let (mut r, mut k) = (c.row, c.col);
        if back {
            if k > 0 {
                k -= 1;
            } else if r > 0 {
                r -= 1;
                k = cols - 1;
            } else {
                return;
            }
        } else if k + 1 < cols {
            k += 1;
        } else if r + 1 < rows {
            r += 1;
            k = 0;
        } else {
            self.push_undo(Kind::Other);
            self.doc.table_mut(&id).unwrap().cells.push(vec![vec![Para::default()]; cols]);
            self.changed = true;
            r += 1;
            k = 0;
        }
        let cell = Cell { row: r, col: k, ..c };
        let n = self.flow(Some(cell)).len();
        let last = self.flow(Some(cell))[n - 1].len();
        // Tab selects the next cell's text, as Notes does
        self.anchor = Loc { cell: Some(cell), para: 0, off: 0 };
        self.head = Loc { cell: Some(cell), para: n - 1, off: last };
    }

    /// Backspace with cells selected: their text goes.
    pub fn clear_table_sel(&mut self) {
        let Some(s) = self.table_sel else { return };
        let Some((_, id)) = self.cur_table() else { return };
        self.push_undo(Kind::Other);
        let t = self.doc.table_mut(&id).unwrap();
        for r in s.r0..=s.r1 {
            for c in s.c0..=s.c1 {
                t.cells[r][c] = vec![Para::default()];
            }
        }
        self.changed = true;
    }

    /// Selected cells as text (rows of tab-separated cells) and as a fragment.
    pub fn table_sel_frag(&self, s: &TableSel) -> Option<(Frag, String)> {
        let id = self.doc.paras.get(s.para)?.sole_att()?;
        let t = self.doc.table(id)?;
        let mut paras = Vec::new();
        let mut lines = Vec::new();
        for r in s.r0..=s.r1.min(t.rows() - 1) {
            let cells: Vec<String> = (s.c0..=s.c1.min(t.cols() - 1)).map(|c| t.cell_text(r, c).replace('\n', " ")).collect();
            let line = cells.join("\t");
            paras.push(Para::new(&line, nt_core::model::ParaStyle::Body));
            lines.push(line);
        }
        Some((Frag { paras, atts: Vec::new(), open_end: false }, lines.join("\n")))
    }
}

/// The handles of the table the caret is in: the row handle left of the caret's row, the column
/// handle above its column.
fn handles(ed: &Editor, pb: &PBox, tb: &TableBox, o: Pos2) -> Option<(Rect, Rect, usize, usize)> {
    let cell = ed.head.cell.filter(|c| c.para == pb.para).or_else(|| ed.table_sel.filter(|s| s.para == pb.para).map(|s| Cell { para: s.para, row: s.r0, col: s.c0 }))?;
    let (ry, rh) = *tb.rows.get(cell.row)?;
    let (cx, cw) = *tb.cols.get(cell.col)?;
    let row = Rect::from_center_size(pos2(o.x - 9.0, o.y + ry + rh / 2.0), vec2(10.0, (rh * 0.62).clamp(14.0, 24.0)));
    let col = Rect::from_center_size(pos2(o.x + cx + cw / 2.0, o.y - 9.0), vec2((cw * 0.18).clamp(20.0, 28.0), 10.0));
    Some((row, col, cell.row, cell.col))
}

/// A press on a row or column handle selects the row or column.
pub fn press_handles(ed: &mut Editor, p: Pos2) -> Option<TableSel> {
    let flow = ed.view.flow.as_ref()?;
    for pb in &flow.boxes {
        let Some(Block::Table(tb)) = &pb.block else { continue };
        let o = ed.view.origin + vec2(pb.x, pb.top);
        let Some((row, col, r, c)) = handles(ed, pb, tb, o) else { continue };
        if row.expand(4.0).contains(p) {
            return Some(TableSel { para: pb.para, r0: r, c0: 0, r1: r, c1: tb.cols.len() - 1 });
        }
        if col.expand(4.0).contains(p) {
            return Some(TableSel { para: pb.para, r0: 0, c0: c, r1: tb.rows.len() - 1, c1: c });
        }
    }
    None
}

/// Where a dragged row or column goes for the pointer at `p`: the neighbour whose middle it
/// has passed (None: where it is).
pub fn reorder_target(ed: &Editor, para: usize, row: bool, at: usize, p: Pos2) -> Option<usize> {
    let pb = ed.view.flow.as_ref()?.boxes.get(para)?;
    let Some(Block::Table(tb)) = &pb.block else { return None };
    let o = ed.view.origin + vec2(pb.x, pb.top);
    let spans: Vec<(f32, f32)> = if row { tb.rows.iter().map(|r| (o.y + r.0, r.1)).collect() } else { tb.cols.iter().map(|c| (o.x + c.0, c.1)).collect() };
    let q = if row { p.y } else { p.x };
    let mut to = at;
    while to + 1 < spans.len() && q > spans[to + 1].0 + spans[to + 1].1 / 2.0 {
        to += 1;
    }
    while to > 0 && q < spans[to - 1].0 + spans[to - 1].1 / 2.0 {
        to -= 1;
    }
    (to != at).then_some(to)
}

impl Editor {
    /// Moves a table's row (or column) `from` to `to`, its selection with it.
    pub fn table_move(&mut self, para: usize, row: bool, from: usize, to: usize) {
        let Some(id) = self.doc.paras.get(para).and_then(|p| p.sole_att()).map(str::to_string) else { return };
        let Some(t) = self.doc.table_mut(&id) else { return };
        if row {
            if from >= t.cells.len() || to >= t.cells.len() {
                return;
            }
            let r = t.cells.remove(from);
            t.cells.insert(to, r);
        } else {
            for r in &mut t.cells {
                if from < r.len() && to < r.len() {
                    let c = r.remove(from);
                    r.insert(to, c);
                }
            }
            if t.widths.len() < t.cols() {
                t.widths.resize(t.cols(), None);
            }
            let w = t.widths.remove(from);
            t.widths.insert(to, w);
        }
        let (rows, cols) = (t.rows(), t.cols());
        self.table_sel = Some(if row { TableSel { para, r0: to, c0: 0, r1: to, c1: cols - 1 } } else { TableSel { para, r0: 0, c0: to, r1: rows - 1, c1: to } });
        // (the caret on the table itself while a whole row or column is selected)
        self.head = Loc { cell: None, para, off: 0 };
        self.anchor = self.head;
        self.changed = true;
    }
}

/// Which handle a point is over: (table paragraph, Some(row) for the row handle / Some(col) for the column one).
pub fn handle_at(ed: &Editor, p: Pos2) -> Option<(usize, Option<usize>, Option<usize>)> {
    let flow = ed.view.flow.as_ref()?;
    for pb in &flow.boxes {
        let Some(Block::Table(tb)) = &pb.block else { continue };
        let o = ed.view.origin + vec2(pb.x, pb.top);
        let Some((row, col, r, c)) = handles(ed, pb, tb, o) else { continue };
        if row.expand(4.0).contains(p) {
            return Some((pb.para, Some(r), None));
        }
        if col.expand(4.0).contains(p) {
            return Some((pb.para, None, Some(c)));
        }
    }
    None
}

pub fn paint_table(ed: &mut Editor, painter: &Painter, pb: &PBox, tb: &TableBox, o: Pos2, clip: Rect) {
    let w = tb.cols.last().map_or(0.0, |c| c.0 + c.1);
    let h = tb.rows.last().map_or(0.0, |r| r.0 + r.1);
    let line = Stroke::new(layout::TABLE_LINE, layout::line_color());
    // cell text
    let flow = ed.view.flow.clone().unwrap_or_default();
    for (r, row) in tb.cells.iter().enumerate() {
        for (c, f) in row.iter().enumerate() {
            let cell = Cell { para: pb.para, row: r, col: c };
            if let Some((_, co)) = ed.flow_geo(&flow, ed.view.origin, Some(cell)) {
                super::paint::paint_flow(ed, painter, f, co, Some(cell), clip);
            }
        }
    }
    // the grid: the outer frame and the lines between rows and columns
    let x0 = o.x + 0.5;
    let y0 = o.y + 0.5;
    for &(ry, _) in tb.rows.iter().skip(1) {
        painter.hline(x0..=x0 + w, y0 + ry, line);
    }
    for &(cx, _) in tb.cols.iter().skip(1) {
        painter.vline(x0 + cx, y0..=y0 + h, line);
    }
    painter.rect_stroke(Rect::from_min_size(pos2(x0, y0), vec2(w, h)), 0.0, line, egui::StrokeKind::Middle);
    // the handles while the caret is in the table
    if let Some((row, col, _, _)) = handles(ed, pb, tb, o) {
        for (r, vertical) in [(row, true), (col, false)] {
            painter.rect_filled(r, CornerRadius::same(5), handle_bg());
            let c = r.center();
            for k in [-1.0f32, 0.0, 1.0] {
                let d = if vertical { vec2(0.0, k * 3.0) } else { vec2(k * 3.0, 0.0) };
                painter.circle_filled(c + d, 1.0, handle_dot());
            }
        }
    }
}

fn handle_bg() -> Color32 {
    if layout::dark() { Color32::from_gray(70) } else { Color32::from_gray(228) }
}

fn handle_dot() -> Color32 {
    if layout::dark() { Color32::from_gray(200) } else { Color32::from_gray(110) }
}

/// The selected rows, columns or cells: a yellow outline and tint, the handle in yellow.
pub fn paint_selection(ed: &Editor, painter: &Painter, flow: &Flow, o: Pos2) {
    let Some(s) = ed.table_sel else { return };
    let Some(pb) = flow.boxes.get(s.para) else { return };
    let Some(Block::Table(tb)) = &pb.block else { return };
    let to = o + vec2(pb.x, pb.top);
    let (Some(&(y0, _)), Some(&(y1, h1)), Some(&(x0, _)), Some(&(x1, w1))) = (tb.rows.get(s.r0), tb.rows.get(s.r1), tb.cols.get(s.c0), tb.cols.get(s.c1)) else { return };
    let r = Rect::from_min_max(to + vec2(x0, y0), to + vec2(x1 + w1, y1 + h1)).translate(vec2(0.5, 0.5));
    painter.rect_filled(r, 0.0, layout::accent().gamma_multiply(0.18));
    painter.rect_stroke(r, 0.0, Stroke::new(2.0, layout::accent()), egui::StrokeKind::Middle);
    if let Some((row, col, _, _)) = handles(ed, pb, tb, to) {
        let whole_rows = s.c0 == 0 && s.c1 + 1 == tb.cols.len();
        let whole_cols = s.r0 == 0 && s.r1 + 1 == tb.rows.len();
        for (hr, on, vertical) in [(row, whole_rows, true), (col, whole_cols, false)] {
            if on {
                painter.rect_filled(hr, CornerRadius::same(5), layout::accent());
                let c = hr.center();
                for k in [-1.0f32, 0.0, 1.0] {
                    let d = if vertical { vec2(0.0, k * 3.0) } else { vec2(k * 3.0, 0.0) };
                    painter.circle_filled(c + d, 1.0, Color32::WHITE);
                }
            }
        }
    }
}

/// Dragging a column's right edge resizes it.
pub fn column_drag(ed: &mut Editor, ui: &egui::Ui, resp: &egui::Response) {
    let ctx = ui.ctx();
    let Some(p) = ctx.input(|i| i.pointer.interact_pos()) else { return };
    let mut over = None;
    if let Some(flow) = &ed.view.flow {
        for pb in &flow.boxes {
            let Some(Block::Table(tb)) = &pb.block else { continue };
            let o = ed.view.origin + vec2(pb.x, pb.top);
            let h = tb.rows.last().map_or(0.0, |r| r.0 + r.1);
            if p.y < o.y || p.y > o.y + h {
                continue;
            }
            for (c, &(cx, cw)) in tb.cols.iter().enumerate() {
                if (p.x - (o.x + cx + cw)).abs() <= 3.0 {
                    over = Some((pb.para, c, cw));
                }
            }
        }
    }
    if over.is_some() || matches!(ed.view.drag, Some(Drag::Column(..))) {
        ctx.set_cursor_icon(egui::CursorIcon::ResizeColumn);
    }
    if ctx.input(|i| i.pointer.primary_pressed()) && resp.hovered() {
        if let Some((para, c, w)) = over {
            ed.view.drag = Some(Drag::Column(para, c, w));
            ed.push_undo(Kind::Other);
        }
    }
    if let Some(Drag::Column(para, c, w0)) = ed.view.drag.clone() {
        if resp.dragged() || ctx.input(|i| i.pointer.primary_down()) {
            let dx = ctx.input(|i| i.pointer.press_origin().map_or(0.0, |o| p.x - o.x));
            let id = ed.doc.paras.get(para).and_then(|p| p.sole_att()).unwrap_or_default().to_string();
            if let Some(t) = ed.doc.table_mut(&id) {
                if t.widths.len() < t.cols() {
                    t.widths.resize(t.cols(), None);
                }
                t.widths[c] = Some((w0 + dx).max(layout::MIN_COL));
                ed.changed = true;
            }
        } else {
            ed.view.drag = None;
        }
    }
}
