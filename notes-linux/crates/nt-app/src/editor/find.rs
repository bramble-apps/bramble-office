//! Edit > Find: the find bar over the note (Find…, Find and Replace…, Find Next / Previous, Use
//! Selection for Find, Jump to Selection), matches marked in yellow, the current one brighter.
//! The bar's Replace checkbox shows the replace row (Replace, All) like Find and Replace….

use egui::{Color32, CornerRadius, Rect, Stroke, pos2, vec2};

use super::{Cell, Editor, Loc};
use super::ops::Kind;

#[derive(Default, Clone, Debug)]
pub struct FindState {
    pub open: bool,
    pub replace: bool,
    pub query: String,
    pub with: String,
    pub current: usize,
    pub focus: bool,
}

/// The find bar's height.
pub const BAR_H: f32 = 34.0;
pub const BAR_H_REPLACE: f32 = 62.0;

impl Editor {
    pub fn find_command(&mut self, name: &str) {
        match name {
            "Find" => {
                self.find.open = true;
                self.find.replace = false;
                self.find.focus = true;
            }
            "Find and Replace" => {
                self.find.open = true;
                self.find.replace = true;
                self.find.focus = true;
            }
            "Find Next" => self.find_step(true),
            "Find Previous" => self.find_step(false),
            "Use Selection for Find" => {
                if let Some((_, t)) = self.copy_frag() {
                    self.find.query = t;
                    self.find.current = 0;
                }
            }
            "Jump to Selection" => self.view.scroll_to_caret = true,
            _ => {}
        }
    }

    /// Every match of the query: (cell, paragraph, byte range), in reading order (case-insensitive).
    pub fn find_matches(&self) -> Vec<(Option<Cell>, usize, usize, usize)> {
        let q = self.find.query.to_lowercase();
        if q.is_empty() || !self.find.open {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut scan = |cell: Option<Cell>, i: usize, t: &str| {
            let low = t.to_lowercase();
            // only when lower-casing keeps the byte offsets
            if low.len() != t.len() {
                return;
            }
            let mut from = 0;
            while let Some(k) = low[from..].find(&q) {
                out.push((cell, i, from + k, from + k + q.len()));
                from += k + q.len().max(1);
            }
        };
        for (i, p) in self.doc.paras.iter().enumerate() {
            if let Some(t) = p.sole_att().and_then(|id| self.doc.table(id)) {
                for r in 0..t.rows() {
                    for c in 0..t.cols() {
                        for (j, cp) in t.cells[r][c].iter().enumerate() {
                            scan(Some(Cell { para: i, row: r, col: c }), j, &cp.text);
                        }
                    }
                }
            } else {
                scan(None, i, &p.text);
            }
        }
        out
    }

    fn find_step(&mut self, next: bool) {
        let m = self.find_matches();
        if m.is_empty() {
            return;
        }
        let n = m.len();
        self.find.current = if next { (self.find.current + 1) % n } else { (self.find.current + n - 1) % n };
        self.select_match(&m);
    }

    fn select_match(&mut self, m: &[(Option<Cell>, usize, usize, usize)]) {
        if let Some(&(cell, para, a, b)) = m.get(self.find.current) {
            self.anchor = Loc { cell, para, off: a };
            self.head = Loc { cell, para, off: b };
            // a collapsed section opens to show it
            let top = cell.map_or(para, |c| c.para);
            if super::layout::hidden_paras(&self.doc.paras)[top] {
                for j in (0..top).rev() {
                    if self.doc.paras[j].is_heading() && self.doc.paras[j].collapsed {
                        self.doc.paras[j].collapsed = false;
                        self.changed = true;
                    }
                }
            }
            self.view.scroll_to_caret = true;
        }
    }

    pub(super) fn replace_one(&mut self) {
        let m = self.find_matches();
        let Some(&(cell, para, a, b)) = m.get(self.find.current) else { return };
        self.push_undo(Kind::Other);
        let with = self.find.with.clone();
        let f = self.flow(cell)[para].fmt_at(a + 1);
        let flow = self.flow_mut(cell);
        flow[para].remove(a..b);
        flow[para].insert(a, &with, &f);
        self.changed = true;
        let m = self.find_matches();
        if !m.is_empty() {
            self.find.current %= m.len();
            self.select_match(&m);
        }
    }

    pub(super) fn replace_all(&mut self) {
        let m = self.find_matches();
        if m.is_empty() {
            return;
        }
        self.push_undo(Kind::Other);
        let with = self.find.with.clone();
        for &(cell, para, a, b) in m.iter().rev() {
            let f = self.flow(cell)[para].fmt_at(a + 1);
            let flow = self.flow_mut(cell);
            flow[para].remove(a..b);
            flow[para].insert(a, &with, &f);
        }
        self.changed = true;
    }
}

/// The bar's height while it is open.
pub fn bar_height(ed: &Editor) -> f32 {
    match (ed.find.open, ed.find.replace) {
        (false, _) => 0.0,
        (true, false) => BAR_H,
        (true, true) => BAR_H_REPLACE,
    }
}

/// Draws the find bar at the top of `rect`.
pub fn bar(ed: &mut Editor, ui: &mut egui::Ui, rect: Rect) {
    if !ed.find.open {
        return;
    }
    let h = bar_height(ed);
    let r = Rect::from_min_size(rect.min, vec2(rect.width(), h));
    let p = ui.painter_at(rect);
    let c = ed.corners;
    p.rect_filled(r, egui::CornerRadius { nw: c.nw, ne: c.ne, sw: 0, se: 0 }, if super::layout::dark() { Color32::from_gray(40) } else { Color32::from_gray(246) });
    p.hline(r.x_range(), r.max.y - 0.5, Stroke::new(1.0, super::layout::line_color().gamma_multiply(0.6)));
    let n = ed.find_matches().len();
    let mut close = false;
    let mut step = None;
    let (mut rep, mut rep_all) = (false, false);
    ui.scope_builder(egui::UiBuilder::new().max_rect(r.shrink2(vec2(10.0, 5.0))), |ui| {
        // (a Replace checkbox on the left, as the Mac's find bar has, turns the replace row on;
        // the replace field lines up under the search field)
        let mut lead = 0.0;
        ui.horizontal(|ui| {
            let x0 = ui.cursor().min.x;
            if kn_ui::theme::checkbox(ui, &mut ed.find.replace, "Replace").changed() && ed.find.replace {
                ed.find.focus = true;
            }
            lead = ui.cursor().min.x - x0;
            let q = ui.add(egui::TextEdit::singleline(&mut SafeText(&mut ed.find.query)).hint_text("Search").desired_width((rect.width() - 260.0).clamp(120.0, 420.0)));
            if std::mem::take(&mut ed.find.focus) {
                q.request_focus();
            }
            if q.changed() {
                ed.find.current = 0;
            }
            if q.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                step = Some(!ui.input(|i| i.modifiers.shift));
                q.request_focus();
            }
            if q.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                close = true;
            }
            ui.label(if ed.find.query.is_empty() { String::new() } else if n == 1 { "1 match".into() } else { format!("{n} matches") });
            if ui.button("\u{2039}").on_hover_text("Previous").clicked() {
                step = Some(false);
            }
            if ui.button("\u{203a}").on_hover_text("Next").clicked() {
                step = Some(true);
            }
            if ui.button("Done").clicked() {
                close = true;
            }
        });
        if ed.find.replace {
            ui.horizontal(|ui| {
                ui.add_space(lead - ui.spacing().item_spacing.x);
                ui.add(egui::TextEdit::singleline(&mut SafeText(&mut ed.find.with)).hint_text("Replace").desired_width((rect.width() - 260.0).clamp(120.0, 420.0)));
                rep = ui.button("Replace").clicked();
                rep_all = ui.button("All").clicked();
            });
        }
    });
    if let Some(next) = step {
        // the first Return goes to the first match after the caret
        ed.find_step(next);
    }
    if rep {
        ed.replace_one();
    }
    if rep_all {
        ed.replace_all();
    }
    if close {
        ed.find.open = false;
    }
}

/// Marks the matches in one paragraph's rows (called while painting it).
pub fn paint_matches(ed: &Editor, painter: &egui::Painter, cell: Option<Cell>, para: usize, g: &egui::Galley, map: &super::layout::Map, base: egui::Pos2) {
    if !ed.find.open || ed.find.query.is_empty() {
        return;
    }
    for (k, (c, i, a, b)) in ed.find_matches().into_iter().enumerate() {
        if c != cell || i != para {
            continue;
        }
        let cur = k == ed.find.current;
        for cr in super::layout::char_rects(g, map.to_char(a), map.to_char(b), true) {
            let rr = cr.translate(base.to_vec2()).expand2(vec2(1.0, -1.0));
            painter.rect_filled(rr, CornerRadius::same(3), if cur { Color32::from_rgb(255, 214, 10) } else { Color32::from_rgba_unmultiplied(255, 230, 110, 150) });
        }
    }
    let _ = pos2;
}

/// A text field's text that can't panic egui: a deletion range it gets backwards or past the
/// end (egui 0.37's word deletion with a stale cursor) is put in order and kept inside the text.
pub struct SafeText<'a>(pub &'a mut String);

impl egui::TextBuffer for SafeText<'_> {
    fn is_mutable(&self) -> bool {
        true
    }

    fn as_str(&self) -> &str {
        self.0.as_str()
    }

    fn insert_text(&mut self, text: &str, at: egui::text::CharIndex) -> usize {
        let n = self.0.chars().count();
        self.0.insert_text(text, egui::text::CharIndex(at.0.min(n)))
    }

    fn delete_char_range(&mut self, r: std::ops::Range<egui::text::CharIndex>) {
        let n = self.0.chars().count();
        let (a, b) = (r.start.0.min(r.end.0).min(n), r.start.0.max(r.end.0).min(n));
        self.0.delete_char_range(egui::text::CharIndex(a)..egui::text::CharIndex(b));
    }

    fn type_id(&self) -> std::any::TypeId {
        std::any::TypeId::of::<SafeText<'static>>()
    }
}
