//! One frame of the editor: lay the note out, scroll, hit-test the mouse (caret, word and
//! paragraph selection, checklist circles, heading chevrons, links, tags, table handles), move the
//! caret with the keys (macOS's: Home / End scroll, ⌥ word, ⌘ line and note edges, here ⌃ for ⌘ and
//! Alt for ⌥ as the suite maps them), type, and copy / paste.

use egui::{Event, Key, Modifiers, Pos2, Rect, Sense, Vec2, pos2, vec2};

use super::doc::{AttKind, OBJ};
use super::layout::{self, Block, Flow, Lx, PBox};
use super::ops::{self, Frag, Kind};
use super::{Cell, Editor, Loc, TableSel};

/// What the mouse is doing.
#[derive(Clone, Debug, PartialEq)]
pub enum Drag {
    /// selecting text from the anchor
    Select,
    /// selecting by words (double-click drag) or paragraphs (triple-click drag), from this range
    Words(Loc, Loc),
    /// across table cells, from this cell
    Cells(Cell),
    /// a table column's right edge: (table paragraph, column, width when the drag began)
    Column(usize, usize, f32),
    /// moving the selected text
    Move,
    /// a checklist item by its handle
    Item(usize),
    /// a table's row (true) or column by its handle: (table paragraph, which, where it is now)
    Reorder(usize, bool, usize),
}

#[derive(Default)]
pub struct View {
    pub scroll: super::scroll::Scroller,
    pub content: Vec2,
    pub flow: Option<Flow>,
    /// the flow's paragraph layouts kept between frames
    pub cache: layout::FlowCache,
    /// room above the flow's first line (the date line), from the last layout
    pub top_pad: f32,
    /// the top of what is drawn: above the content's top by the toolbar strip
    pub clip_top: f32,
    /// where the date line was drawn (the toolbar's material tints with it)
    pub date_rect: Option<Rect>,
    /// the screen position of the body flow's top left
    pub origin: Pos2,
    pub rect: Option<Rect>,
    pub scroll_to_caret: bool,
    pub paste_match: bool,
    /// the next paste is Markdown (Edit > Paste as Markdown)
    pub paste_md: bool,
    /// a row or column drag has its undo step
    pub reorder_undo: bool,
    pub drag: Option<Drag>,
    pub clicks: u32,
    pub last_click: (f64, Pos2),
    pub blink: f64,
    /// the paragraph under the mouse (heading chevrons, checklist drag handles)
    pub hover: Option<usize>,
    /// Math Results (Suggest): the paragraph and the result shown after its "="
    pub ghost: Option<(Loc, String)>,
    /// the # / >> autocomplete: where it started and what is typed after it
    pub complete: Option<Complete>,
    pub spell: std::collections::HashMap<String, Vec<(usize, usize)>>,
    /// Show Spelling and Grammar's panel, and the guess picked in it
    /// data detectors' finds by paragraph text
    pub detect: std::collections::HashMap<String, Vec<(usize, usize, super::detect::Kind)>>,
    /// paragraphs being checked on the workers
    pub spell_asked: std::collections::HashSet<String>,
    pub spell_panel: bool,
    /// Show Fonts' and Show Colors' panels
    pub fonts_panel: bool,
    pub colors_panel: bool,
    pub spell_pick: Option<String>,
    /// a circle just ticked: (cell, paragraph, when)
    pub tick_anim: Option<(String, f64)>,
    /// items sliding to a new place: item uuid -> (offset from its new place when it began, when)
    /// sliding items: how far from their place, when the slide starts, how long it takes
    pub slides: std::collections::HashMap<String, (f32, f64, f64)>,
    /// while dragging an item: where it would go (before this paragraph)
    pub drop_at: Option<usize>,
    pub id: Option<egui::Id>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Complete {
    pub at: Loc,
    /// '#' or '>'
    pub kind: char,
    pub query: String,
    pub pick: usize,
}

thread_local! {
    static CLIP: std::cell::RefCell<Option<(Frag, String)>> = const { std::cell::RefCell::new(None) };
}

/// Puts a fragment on the clipboard: plain text for other apps, the fragment for pasting here.
pub fn set_clipboard(ctx: Option<&egui::Context>, frag: Frag, plain: String) {
    if let Some(ctx) = ctx {
        ctx.copy_text(plain.clone());
    }
    CLIP.with_borrow_mut(|c| *c = Some((frag, plain)));
}

/// The fragment copied here, if the clipboard still holds its text.
fn our_clip(text: &str) -> Option<Frag> {
    CLIP.with_borrow(|c| c.as_ref().filter(|(_, p)| p.replace("\r\n", "\n") == text.replace("\r\n", "\n")).map(|(f, _)| f.clone()))
}

impl Editor {
    fn lx<'a>(&'a self, ctx: &'a egui::Context, media: &'a dyn Fn(&str) -> Option<Vec2>) -> Lx<'a> {
        Lx { ctx, doc: &self.doc, scale: self.settings.text_scale, media }
    }

    // ---- geometry

    /// The flow a position is in and its screen origin.
    pub(crate) fn flow_geo<'a>(&self, flow: &'a Flow, origin: Pos2, cell: Option<Cell>) -> Option<(&'a Flow, Pos2)> {
        let Some(c) = cell else { return Some((flow, origin)) };
        let pb = flow.boxes.get(c.para)?;
        let Some(Block::Table(tb)) = &pb.block else { return None };
        let f = tb.cells.get(c.row)?.get(c.col)?;
        let s = self.settings.text_scale;
        Some((f, origin + vec2(pb.x + tb.cols[c.col].0 + layout::CELL_PAD_X * s, pb.top + tb.rows[c.row].0 + layout::CELL_PAD_Y * s)))
    }

    /// The caret's screen rectangle at a position.
    pub(crate) fn caret_screen(&self, l: Loc) -> Option<Rect> {
        let flow = self.view.flow.as_ref()?;
        let (f, o) = self.flow_geo(flow, self.view.origin, l.cell)?;
        let pb = f.boxes.get(l.para)?;
        let base = o + vec2(pb.galley_x, pb.top + pb.gy);
        match &pb.galley {
            Some(g) => {
                let ch = pb.map.to_char(l.off);
                Some(layout::caret_rect(g, ch).translate(base.to_vec2()))
            }
            None => {
                let w = match &pb.block {
                    Some(Block::Table(tb)) => tb.cols.last().map_or(0.0, |c| c.0 + c.1),
                    Some(Block::Media(s)) => s.x,
                    _ => pb.width,
                };
                let x = if l.off == 0 { base.x - 2.0 } else { base.x + w + 2.0 };
                Some(Rect::from_min_max(pos2(x, base.y), pos2(x, base.y + pb.height.max(pb.size))))
            }
        }
    }

    /// The position nearest a screen point in a flow (descending into table cells when `cells`).
    pub(crate) fn hit_flow(&self, flow: &Flow, origin: Pos2, p: Pos2, cell: Option<Cell>, cells: bool) -> Loc {
        let shown: Vec<&PBox> = flow.boxes.iter().filter(|b| !b.hidden).collect();
        let Some(first) = shown.first() else { return Loc { cell, para: 0, off: 0 } };
        let y = p.y - origin.y;
        let pb = shown.iter().find(|b| y < b.bottom()).copied().unwrap_or(shown.last().unwrap());
        let pb = if y < first.top { first } else { pb };
        let base = origin + vec2(pb.galley_x, pb.top + pb.gy);
        if let Some(g) = &pb.galley {
            let ch = layout::hit(g, p - base);
            return Loc { cell, para: pb.para, off: pb.map.to_byte(ch) };
        }
        if let (true, None, Some(Block::Table(tb))) = (cells, cell, &pb.block) {
            let tp = p - (origin + vec2(pb.x, pb.top));
            let in_table = tp.y >= 0.0 && tp.y < pb.height && tp.x >= -8.0 && tp.x <= tb.cols.last().map_or(0.0, |c| c.0 + c.1) + 8.0;
            if in_table {
                let r = tb.rows.iter().position(|&(ry, rh)| tp.y < ry + rh).unwrap_or(tb.rows.len().saturating_sub(1));
                let c = tb.cols.iter().position(|&(cx, cw)| tp.x < cx + cw).unwrap_or(tb.cols.len().saturating_sub(1));
                let cl = Cell { para: pb.para, row: r, col: c };
                if let Some((f, o)) = self.flow_geo(flow, origin, Some(cl)) {
                    return self.hit_flow(f, o, p, Some(cl), false);
                }
            }
        }
        let len = self.flow(cell).get(pb.para).map_or(0, |q| q.len());
        let w = match &pb.block {
            Some(Block::Media(s)) => s.x,
            _ => pb.width,
        };
        Loc { cell, para: pb.para, off: if p.x < base.x + w / 2.0 { 0 } else { len } }
    }

    pub(crate) fn hit(&self, p: Pos2, cells: bool) -> Option<Loc> {
        let flow = self.view.flow.as_ref()?;
        Some(self.hit_flow(flow, self.view.origin, p, None, cells))
    }

    /// The table under a point: (paragraph, row, column, the table's screen origin).
    pub(crate) fn table_at(&self, p: Pos2) -> Option<(usize, Option<usize>, Option<usize>, Pos2)> {
        let flow = self.view.flow.as_ref()?;
        for pb in &flow.boxes {
            let Some(Block::Table(tb)) = &pb.block else { continue };
            let o = self.view.origin + vec2(pb.x, pb.top);
            let w = tb.cols.last().map_or(0.0, |c| c.0 + c.1);
            let h = tb.rows.last().map_or(0.0, |r| r.0 + r.1);
            let area = Rect::from_min_size(o - vec2(16.0, 16.0), vec2(w + 32.0, h + 32.0));
            if area.contains(p) {
                let t = p - o;
                let r = tb.rows.iter().position(|&(ry, rh)| t.y >= ry && t.y < ry + rh);
                let c = tb.cols.iter().position(|&(cx, cw)| t.x >= cx && t.x < cx + cw);
                return Some((pb.para, r, c, o));
            }
        }
        None
    }

    // ---- caret movement

    pub fn move_h(&mut self, right: bool, word: bool, extend: bool) {
        if self.has_sel() && !extend {
            let (a, b) = self.range();
            let l = if right { b } else { a };
            self.set_caret(l, false);
            return;
        }
        let mut l = self.head;
        let flow = self.flow(l.cell);
        let t = &flow[l.para].text;
        if right {
            if l.off < t.len() {
                l.off = if word { ops::next_word(t, l.off) } else { ops::next_char(t, l.off) };
            } else if let Some(n) = self.next_shown(l.cell, l.para, true) {
                l = Loc { para: n, off: 0, ..l };
            }
        } else if l.off > 0 {
            l.off = if word { ops::prev_word(t, l.off) } else { ops::prev_char(t, l.off) };
        } else if let Some(n) = self.next_shown(l.cell, l.para, false) {
            l = Loc { para: n, off: self.flow(l.cell)[n].len(), ..l };
        }
        self.set_caret(l, extend);
    }

    /// The next (or previous) paragraph not hidden in a collapsed section.
    fn next_shown(&self, cell: Option<Cell>, i: usize, down: bool) -> Option<usize> {
        let flow = self.flow(cell);
        let hidden = if cell.is_none() { layout::hidden_paras(flow) } else { vec![false; flow.len()] };
        if down { (i + 1..flow.len()).find(|&j| !hidden[j]) } else { (0..i).rev().find(|&j| !hidden[j]) }
    }

    /// Up / Down: the line above or below at the same x (into and out of table cells).
    pub fn move_v(&mut self, down: bool, extend: bool) {
        let Some(c) = self.caret_screen(self.head) else { return };
        let x = *self.goal_x.get_or_insert(c.center().x);
        let goal = self.goal_x;
        let y = if down { c.max.y + 2.0 } else { c.min.y - 2.0 };
        let target = match self.head.cell {
            Some(cell) => {
                let flow = self.view.flow.as_ref().unwrap();
                let (f, o) = self.flow_geo(flow, self.view.origin, Some(cell)).unwrap();
                if (o.y..=o.y + f.height).contains(&y) {
                    self.hit_flow(f, o, pos2(x, y), Some(cell), false)
                } else {
                    // out of the cell: the cell above / below, or out of the table
                    let id = self.doc.paras[cell.para].sole_att().unwrap_or_default().to_string();
                    let rows = self.doc.table(&id).map_or(0, |t| t.rows());
                    if down && cell.row + 1 < rows {
                        let nc = Cell { row: cell.row + 1, ..cell };
                        let (f, o) = self.flow_geo(flow, self.view.origin, Some(nc)).unwrap();
                        self.hit_flow(f, o, pos2(x, o.y), Some(nc), false)
                    } else if !down && cell.row > 0 {
                        let nc = Cell { row: cell.row - 1, ..cell };
                        let (f, o) = self.flow_geo(flow, self.view.origin, Some(nc)).unwrap();
                        self.hit_flow(f, o, pos2(x, o.y + f.height - 1.0), Some(nc), false)
                    } else if down {
                        let n = (cell.para + 1).min(self.doc.paras.len() - 1);
                        Loc { cell: None, para: n, off: if n == cell.para { self.doc.paras[n].len() } else { 0 } }
                    } else if cell.para > 0 {
                        let n = cell.para - 1;
                        self.hit(pos2(x, self.caret_screen(Loc { cell: None, para: n, off: self.doc.paras[n].len() }).map_or(y, |r| r.center().y)), false).unwrap_or(self.head)
                    } else {
                        Loc { cell: None, para: 0, off: 0 }
                    }
                }
            }
            None => {
                let body_h = self.view.flow.as_ref().map_or(0.0, |f| f.height);
                if !down && y < self.view.origin.y {
                    Loc { cell: None, para: 0, off: 0 }
                } else if down && y > self.view.origin.y + body_h {
                    let n = self.doc.paras.len() - 1;
                    Loc { cell: None, para: n, off: self.doc.paras[n].len() }
                } else {
                    self.hit(pos2(x, y), true).unwrap_or(self.head)
                }
            }
        };
        self.set_caret(target, extend && target.cell == self.anchor.cell);
        self.goal_x = goal;
    }

    /// ⌘← / ⌘→: the line's start or end.
    fn line_edge(&mut self, end: bool, extend: bool) {
        let Some(c) = self.caret_screen(self.head) else { return };
        let flow = self.view.flow.as_ref().unwrap();
        let Some((f, o)) = self.flow_geo(flow, self.view.origin, self.head.cell) else { return };
        let x = if end { o.x + 10000.0 } else { o.x - 10000.0 };
        let mut l = self.hit_flow(f, o, pos2(x, c.center().y), self.head.cell, false);
        l.para = self.head.para;
        self.set_caret(l, extend);
    }

    fn doc_edge(&mut self, end: bool, extend: bool) {
        let cell = self.head.cell;
        let flow = self.flow(cell);
        let l = if end { Loc { cell, para: flow.len() - 1, off: flow[flow.len() - 1].len() } } else { Loc { cell, para: 0, off: 0 } };
        self.set_caret(l, extend);
    }

    /// Return: accepts a math suggestion, closes the autocomplete with its pick, else a new paragraph.
    pub fn return_key(&mut self) {
        if self.accept_math() {
            return;
        }
        if self.accept_complete() {
            return;
        }
        self.new_paragraph();
    }

    /// Tab: next table cell (a new row after the last), else a math suggestion, else indents a
    /// list item, else a tab character.
    pub fn tab_key(&mut self, back: bool) {
        if self.head.cell.is_some() {
            self.table_tab(back);
            return;
        }
        if !back && (self.accept_math() || self.accept_complete()) {
            return;
        }
        let p = self.para(self.head);
        if p.is_list() || (back && p.indent > 0) {
            self.indent(if back { -1 } else { 1 });
            return;
        }
        if !back {
            self.type_text("\t");
        }
    }

    fn accept_math(&mut self) -> bool {
        let Some((at, result)) = self.view.ghost.clone() else { return false };
        if at != self.head || self.has_sel() {
            return false;
        }
        self.push_undo(Kind::Other);
        let t = self.para(at).text.clone();
        let eq = t[..at.off].rfind('=').unwrap_or(at.off);
        let flow = self.flow_mut(at.cell);
        flow[at.para].remove(eq..at.off);
        self.head = Loc { off: eq, ..at };
        self.anchor = self.head;
        let alt = format!("\u{200e}\u{2009}=\u{2009}{result}");
        self.insert_token(eq..eq, AttKind::Math(alt), nt_core::model::UTI_MATH);
        self.view.ghost = None;
        true
    }

    /// Results already in the note follow their expression and the variables above them (a
    /// changed "x = 5" changes "x*2 = 10" below it), as Notes' do.
    pub fn refresh_math(&mut self) {
        let mut fix: Vec<(String, String)> = Vec::new();
        for (i, p) in self.doc.paras.iter().enumerate() {
            let mut off = 0;
            for r in &p.runs {
                let at = off;
                off += r.len;
                let Some(id) = r.fmt.att.as_deref() else { continue };
                let Some(super::doc::AttKind::Math(alt)) = self.doc.att(id).map(|a| &a.kind) else { continue };
                let before = p.text[..at].replace(super::doc::OBJ, "");
                let expr = before.rsplit('=').next().unwrap_or("");
                let vars = super::math::variables(&self.doc, Loc { cell: None, para: i, off: 0 });
                if let Some(v) = super::math::result(expr, &vars) {
                    let new = format!("\u{200e}\u{2009}=\u{2009}{v}");
                    if *alt != new {
                        fix.push((id.to_string(), new));
                    }
                }
            }
        }
        for (id, new) in fix {
            if let Some(a) = self.doc.att_mut(&id) {
                a.kind = super::doc::AttKind::Math(new);
            }
            self.changed = true;
        }
    }

    fn accept_complete(&mut self) -> bool {
        let Some(c) = self.view.complete.clone() else { return false };
        let items = self.complete_items(&c);
        let Some(pick) = items.get(c.pick.min(items.len().saturating_sub(1))).cloned() else {
            self.view.complete = None;
            return false;
        };
        self.push_undo(Kind::Other);
        let l = self.head;
        let start = c.at.off;
        match c.kind {
            '#' => self.insert_token(start..l.off, AttKind::Tag(format!("#{}", pick.0)), nt_core::model::UTI_HASHTAG),
            _ => {
                let url = nt_core::model::Token::note_link_url(&pick.1, None);
                self.insert_token(start..l.off, AttKind::NoteLink { text: pick.0.clone(), url }, nt_core::model::UTI_NOTE_LINK)
            }
        }
        let f = self.typing_fmt();
        let off = self.head.off;
        self.flow_mut(l.cell)[l.para].insert(off, " ", &f.typing());
        self.head.off += 1;
        self.anchor = self.head;
        self.view.complete = None;
        true
    }

    /// The autocomplete's choices: (name, note id) — tags for #, note titles for >>.
    pub(crate) fn complete_items(&self, c: &Complete) -> Vec<(String, String)> {
        let q = c.query.to_lowercase();
        if c.kind == '#' {
            let mut v: Vec<(String, String)> = self.tags.iter().filter(|t| t.to_lowercase().starts_with(&q) && !q.is_empty()).map(|t| (t.trim_start_matches('#').to_string(), String::new())).collect();
            v.truncate(8);
            v
        } else {
            let mut v: Vec<(String, String)> = self.notes.iter().filter(|n| Some(n.id.as_str()) != self.note.as_deref() && (q.is_empty() || n.title.to_lowercase().contains(&q))).map(|n| (n.title.clone(), n.id.clone())).collect();
            v.truncate(8);
            v
        }
    }

    /// After typing: what the autocomplete and math suggestion show.
    fn update_suggestions(&mut self) {
        let l = self.head;
        if self.has_sel() {
            self.view.complete = None;
            self.view.ghost = None;
            return;
        }
        let t = self.para(l).text[..l.off].to_string();
        // >> note links and # tags
        let word_start = super::ops::after_last_space(&t);
        let w = &t[word_start..];
        self.view.complete = if let Some(q) = w.strip_prefix(">>") {
            Some(Complete { at: Loc { off: word_start, ..l }, kind: '>', query: q.to_string(), pick: self.view.complete.as_ref().map_or(0, |c| c.pick) })
        } else if let Some(q) = w.strip_prefix('#').filter(|q| !q.is_empty() && q.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-')) {
            let c = Complete { at: Loc { off: word_start, ..l }, kind: '#', query: q.to_string(), pick: 0 };
            (!self.complete_items(&c).is_empty()).then_some(c)
        } else {
            None
        };
        // Math Results: "expr=" suggests its value
        self.view.ghost = None;
        if self.settings.math >= 1 && t.ends_with('=') && !t.contains(OBJ) {
            let vars = super::math::variables(&self.doc, l);
            let expr = &t[..t.len() - 1];
            if let Some(r) = super::math::result(expr, &vars) {
                if self.settings.math == 2 {
                    self.view.ghost = Some((l, r));
                    self.accept_math();
                } else {
                    self.view.ghost = Some((l, r));
                }
            }
        }
    }

    // ---- keyboard

    fn key(&mut self, ctx: &egui::Context, key: Key, m: Modifiers) {
        let cmd = m.command || m.ctrl;
        let word = m.alt;
        match key {
            Key::ArrowLeft | Key::ArrowRight if self.view.complete.is_some() && !m.shift => {
                self.view.complete = None;
                self.move_h(key == Key::ArrowRight, false, false);
            }
            Key::ArrowLeft | Key::ArrowRight => {
                let right = key == Key::ArrowRight;
                if cmd {
                    self.line_edge(right, m.shift);
                } else {
                    self.move_h(right, word, m.shift);
                }
            }
            Key::ArrowUp | Key::ArrowDown if self.view.complete.is_some() => {
                let c = self.view.complete.as_mut().unwrap();
                c.pick = if key == Key::ArrowDown { c.pick + 1 } else { c.pick.saturating_sub(1) };
                let n = self.view.complete.clone().map_or(0, |c| self.complete_items(&c).len());
                if let Some(c) = self.view.complete.as_mut() {
                    c.pick = c.pick.min(n.saturating_sub(1));
                }
            }
            Key::ArrowUp | Key::ArrowDown => {
                let down = key == Key::ArrowDown;
                if cmd && m.ctrl && m.command {
                    self.move_item(down);
                } else if cmd {
                    self.doc_edge(down, m.shift);
                } else if word {
                    // ⌥↑/↓: the paragraph's start / end
                    let mut l = self.head;
                    l.off = if down { self.para(l).len() } else { 0 };
                    self.set_caret(l, m.shift);
                } else {
                    self.move_v(down, m.shift);
                }
            }
            // macOS: Home / End / Page keys scroll and leave the caret where it is
            Key::Home => {
                self.view.scroll.stop();
                self.view.scroll.offset = 0.0
            }
            Key::End => {
                self.view.scroll.stop();
                self.view.scroll.offset = f32::MAX
            }
            Key::PageUp => self.view.scroll.offset -= self.view.rect.map_or(400.0, |r| r.height()) * 0.9,
            Key::PageDown => self.view.scroll.offset += self.view.rect.map_or(400.0, |r| r.height()) * 0.9,
            Key::Backspace => self.backspace(word || (cmd && false)),
            Key::Delete => self.delete_forward(word),
            Key::Enter => {
                if m.shift {
                    // a line break: Notes makes a new paragraph too
                    self.new_paragraph();
                } else {
                    self.return_key();
                }
            }
            Key::Tab => self.tab_key(m.shift),
            Key::Escape => {
                if self.view.complete.take().is_none() {
                    self.table_sel = None;
                    self.view.ghost = None;
                }
            }
            Key::A if cmd && !m.shift => self.select_all(),
            Key::Z if cmd => {
                if m.shift {
                    self.redo()
                } else {
                    self.undo()
                }
            }
            _ => {
                let _ = ctx;
                return;
            }
        }
        self.view.blink = ctx.input(|i| i.time);
        self.view.scroll_to_caret = true;
    }

    fn paste(&mut self, text: &str) {
        let match_style = std::mem::take(&mut self.view.paste_match);
        if std::mem::take(&mut self.view.paste_md) {
            self.paste_frag(&super::markdown::markdown_to_frag(text));
            self.view.scroll_to_caret = true;
            return;
        }
        match our_clip(text).filter(|_| !match_style) {
            Some(frag) => self.paste_frag(&frag),
            None => self.paste_plain(text),
        }
        self.view.scroll_to_caret = true;
    }
}

/// One frame: layout, input, drawing.
pub fn frame(ed: &mut Editor, ui: &mut egui::Ui, rect: Rect) {
    // (under the toolbar strip only drawing: the content, its input and scrolling start below it)
    ed.view.clip_top = rect.min.y;
    let rect = Rect::from_min_max(pos2(rect.min.x, rect.min.y + ed.top_inset.min(rect.height())), rect.max);
    let ctx = ui.ctx().clone();
    ed.now = ctx.input(|i| i.time);
    // (this editor's paper: each window sets its own before drawing)
    layout::set_paper(ed.light_paper);
    let automated = std::env::var_os("NT_SNAP").is_some() || std::env::var_os("NT_EDITOR_SNAP").is_some();
    layout::install_faces(&ctx, automated);
    let id = ui.scope_id().with("nt-editor");
    ed.view.id = Some(id);
    let s = ed.settings.text_scale;
    ed.media.poll(&ctx);
    super::spelling::poll(&mut ed.view);
    // PDF attachments: their pages' sizes from the file itself, read off the UI thread (the
    // sizes stored with a note can be wrong: Apple's scans store a 612 x 816 page for a
    // business card)
    for (path, pages) in ed.media.pdf_pages_read() {
        for a in &mut ed.doc.atts {
            if let AttKind::Pdf { path: Some(p), pages: pg, .. } = &mut a.kind {
                if *p == path && !pages.is_empty() {
                    *pg = pages.clone();
                }
            }
        }
    }
    for a in &ed.doc.atts {
        if let AttKind::Pdf { path: Some(p), .. } = &a.kind {
            ed.media.want_pdf_pages(&ctx, p);
        }
    }

    // ---- layout
    let width = (rect.width() - 2.0 * layout::MARGIN_X * s).max(80.0);
    let media_sizes = ed.media.sizes();
    let size_of = |id: &str| media_sizes.get(id).copied();
    // (where the paragraph at the view's top is, so heights found above it don't move the view)
    let anchor = ed.view.flow.as_ref().filter(|_| ed.view.slides.is_empty()).and_then(|f| {
        let top = ed.view.scroll.offset - ed.view.top_pad;
        f.boxes.iter().find(|b| !b.hidden && b.bottom() > top).map(|b| (b.para, b.top, f.boxes.len()))
    });
    let mut flow = layout_note(ed, &ctx, &size_of, width, rect.height(), LAYOUT_BUDGET);
    if let Some((para, top, _)) = anchor.filter(|a| a.2 == flow.boxes.len()) {
        let dy = flow.boxes[para].top - top;
        if dy.abs() > 0.01 {
            ed.view.scroll.offset += dy;
        }
    }
    let first_base = flow.boxes.iter().find(|b| !b.hidden).map_or(0.0, |b| b.baseline() - b.top);
    let mut top_pad = (layout::DATE_BASELINE + layout::FIRST_BASELINE) * s - first_base;
    ed.view.top_pad = top_pad;
    let mut content_h = top_pad + flow.height + 60.0 * s;
    let max_scroll = (content_h - rect.height()).max(0.0);
    // ---- scrolling
    // the suite's scrolling: GTK wheel notches eased, trackpad as it comes, then a glide
    let hovered = ui.rect_contains_pointer(rect) && !kn_ui::pointer_over_window(&ctx);
    // touch (kn_ui::touch, as Letters): one finger scrolls the note and coasts, a tap clicks,
    // touch and hold right-clicks; while drawing, a finger on the drawing draws
    kn_ui::touch::pan_area(rect);
    if let Some((r, _)) = super::draw::canvas(ed).filter(|_| ed.draw.is_some()) {
        kn_ui::touch::grab(r);
    }
    // a PDF under the pointer takes sideways scrolling (and Shift and the wheel); up and down
    // is the note's
    ed.view.scroll.skip_shift = super::pdf::scroll(ed, &ctx);
    ed.view.scroll.input(&ctx, hovered, rect.height() * 0.9);
    ed.view.scroll.clamp(max_scroll);
    // scrolled to paragraphs still estimated (a jump, the scrollbar): laid out before drawing
    {
        let top = ed.view.scroll.offset - top_pad;
        let bottom = top + rect.height();
        if flow.boxes.iter().any(|b| b.bottom() > top && b.top < bottom && !b.hidden && b.galley.is_none() && b.block.is_none()) {
            flow = layout_note(ed, &ctx, &size_of, width, rect.height(), std::time::Duration::ZERO);
            let first_base = flow.boxes.iter().find(|b| !b.hidden).map_or(0.0, |b| b.baseline() - b.top);
            top_pad = (layout::DATE_BASELINE + layout::FIRST_BASELINE) * s - first_base;
            ed.view.top_pad = top_pad;
            content_h = top_pad + flow.height + 60.0 * s;
        }
    }
    let bar_h = super::find::bar_height(ed);
    let full = rect;
    let rect = Rect::from_min_max(pos2(rect.min.x, rect.min.y + bar_h), rect.max);
    let mut origin = pos2(rect.left() + layout::MARGIN_X * s, rect.top() + top_pad - ed.view.scroll.offset);
    slides(ed, &mut flow, ctx.input(|i| i.time));
    ed.view.flow = Some(flow);
    ed.view.origin = origin;
    ed.view.rect = Some(rect);
    ed.view.content = vec2(rect.width(), content_h);
    // ---- mouse
    let resp = ui.interact(rect, id, Sense::click_and_drag());
    if resp.has_focus() {
        ui.memory_mut(|m| m.set_focus_lock_filter(id, egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true }));
    }
    if ui.rect_contains_pointer(rect) {
        ed.drop_files(&ctx);
    }
    super::table::column_drag(ed, ui, &resp);
    // the overlay scrollbar's strip takes the pointer when the note is longer than the pane
    let track = Rect::from_min_max(pos2(rect.max.x - 14.0, rect.min.y + 4.0), pos2(rect.max.x - 2.0, rect.max.y - 4.0));
    let on_bar = content_h > rect.height() + 1.0 && ctx.input(|i| i.pointer.interact_pos()).is_some_and(|p| track.contains(p));
    let drawing_took = !ed.blocked && super::draw::input(ed, ui, &resp);
    if (!on_bar || ed.view.drag.is_some()) && !drawing_took && !ed.blocked {
        mouse(ed, ui, &resp);
    }
    // ---- keyboard (only when the editor has the keyboard)
    // (while drawing, keys go to the drawing)
    let has_kb = ed.focused && !ed.blocked && ed.draw.is_none() && (resp.has_focus() || ui.memory(|m| m.focused().is_none()));
    if has_kb {
        let events = ctx.input(|i| i.events.clone());
        let mut typed = false;
        for e in events {
            match e {
                Event::Text(t) if !t.chars().all(|c| c.is_control()) => {
                    if ctx.input(|i| i.modifiers.command || i.modifiers.ctrl) {
                        continue;
                    }
                    ed.type_text(&t);
                    typed = true;
                }
                Event::Ime(egui::ImeEvent::Commit(t)) => {
                    ed.type_text(&t);
                    typed = true;
                }
                Event::Key { key, pressed: true, modifiers, .. } => {
                    ed.key(&ctx, key, modifiers);
                    if matches!(key, Key::Backspace | Key::Delete) {
                        typed = true;
                    }
                }
                Event::Copy => ed.command(Some(&ctx), super::Cmd::Copy),
                Event::Cut => ed.command(Some(&ctx), super::Cmd::Cut),
                Event::Paste(t) => ed.paste(&t),
                _ => {}
            }
        }
        if typed {
            ed.update_suggestions();
            ed.view.blink = ctx.input(|i| i.time);
            ed.view.scroll_to_caret = true;
        }
        // where an input method's window goes
        if let Some(c) = ed.caret_screen(ed.head) {
            ctx.output_mut(|o| o.ime = Some(egui::output::IMEOutput { purpose: egui::IMEPurpose::Normal, rect, cursor_rect: c, should_interrupt_composition: false }));
        }
    }
    // the text changed: lay out again before drawing
    if ed.changed || ed.view.scroll_to_caret {
        let mut flow = layout_note(ed, &ctx, &size_of, width, rect.height(), std::time::Duration::ZERO);
        slides(ed, &mut flow, ctx.input(|i| i.time));
        let first_base = flow.boxes.iter().find(|b| !b.hidden).map_or(0.0, |b| b.baseline() - b.top);
        let top_pad = (layout::DATE_BASELINE + layout::FIRST_BASELINE) * s - first_base;
        ed.view.top_pad = top_pad;
        let content_h = top_pad + flow.height + 60.0 * s;
        ed.view.content = vec2(rect.width(), content_h);
        ed.view.flow = Some(flow);
        origin = pos2(rect.left() + layout::MARGIN_X * s, rect.top() + top_pad - ed.view.scroll.offset);
        ed.view.origin = origin;
        if std::mem::take(&mut ed.view.scroll_to_caret) {
            if let Some(c) = ed.caret_screen(ed.head) {
                let pad = 20.0 * s;
                if c.max.y > rect.bottom() - pad {
                    ed.view.scroll.offset += c.max.y - (rect.bottom() - pad);
                } else if c.min.y < rect.top() + pad {
                    ed.view.scroll.offset -= (rect.top() + pad) - c.min.y;
                }
                let max_scroll = (content_h - rect.height()).max(0.0);
                ed.view.scroll.clamp(max_scroll);
                origin = pos2(rect.left() + layout::MARGIN_X * s, rect.top() + top_pad - ed.view.scroll.offset);
                ed.view.origin = origin;
            }
        }
    }
    if std::mem::take(&mut ed.request_repaint) {
        ctx.request_repaint();
    }
    // ---- drawing
    ed.media.pdf_hits.clear();
    ed.media.audio_hits.clear();
    super::paint::paint(ed, ui, rect, has_kb);
    // heights still estimated: found a budget's worth a frame
    if ed.view.cache.estimates() {
        ctx.request_repaint();
    }
    let ch = ed.view.content.y;
    ed.view.scroll.scrollbar(ui, track, ch, rect.height(), id.with("scrollbar"));
    super::find::bar(ed, ui, full);
    super::markup::toolbar(ed, ui);
    super::pdf::overlays(ed, ui);
    super::spelling::panel(ed, &ctx);
    super::fontpanel::panels(ed, &ctx);
    if !ed.blocked {
        super::menu::context(ed, ui, &resp);
    }
}

/// How long a frame may spend laying out paragraphs out of view (whose heights are estimated
/// until then).
const LAYOUT_BUDGET: std::time::Duration = std::time::Duration::from_micros(2500);

/// The note's flow: paragraphs in view (a screen above and below) and the caret's laid out, the
/// rest from last frame's layouts or estimated, `budget` spent on those.
fn layout_note(ed: &mut Editor, ctx: &egui::Context, size_of: &dyn Fn(&str) -> Option<Vec2>, width: f32, view_h: f32, budget: std::time::Duration) -> Flow {
    let top = ed.view.scroll.offset - ed.view.top_pad;
    let must = [0, 1, ed.head.para, ed.anchor.para, ed.head.para + 1, ed.head.para.saturating_sub(1)];
    let mut cache = std::mem::take(&mut ed.view.cache);
    let flow = {
        let lx = ed.lx(ctx, size_of);
        let win = layout::Window { cache: &mut cache, view: (top - 40.0 - ed.top_inset, top + view_h + 40.0), near: (top - view_h, top + 2.0 * view_h), must: &must, budget };
        // (NT_NO_CACHE: everything laid out every frame, for comparing pictures with)
        if std::env::var_os("NT_NO_CACHE").is_some() {
            layout::layout_flow(&lx, &ed.doc.paras, width, true)
        } else {
            layout::layout_flow_with(&lx, &ed.doc.paras, width, true, Some(win))
        }
    };
    ed.view.cache = cache;
    flow
}

/// How long a checklist item takes to slide to its new place.
pub const SLIDE_SECS: f64 = 0.28;

/// Items an edit moved start where they were drawn and slide to their new place (eased out);
/// the model has its new order at once, only the drawing (and hit testing) follows it.
fn slides(ed: &mut Editor, flow: &mut Flow, now: f64) {
    let moved = std::mem::take(&mut ed.moved);
    if let Some(prev) = ed.view.flow.as_ref() {
        for (cell, id, old) in moved {
            if cell.is_some() {
                continue;
            }
            let (Some(ob), Some(new)) = (prev.boxes.get(old), ed.doc.paras.iter().position(|p| p.check_id == id)) else { continue };
            let Some(nb) = flow.boxes.get(new) else { continue };
            let dy = ob.top - nb.top;
            if dy.abs() > 0.5 {
                // one already sliding keeps moving (no stalled frame): its new slide is a frame in;
                // Move Checked to Bottom waits 0.25 s, then slides for 0.317 s (NO2)
                let (delay, dur) = ed.slide_timing.unwrap_or((0.0, SLIDE_SECS));
                let t0 = if ed.view.slides.contains_key(&id) { now - 1.0 / 60.0 } else { now + delay };
                ed.view.slides.insert(id, (dy, t0, dur));
            }
        }
    }
    ed.slide_timing = None;
    if ed.view.slides.is_empty() {
        return;
    }
    ed.view.slides.retain(|_, (_, t0, dur)| now - *t0 < *dur);
    for b in &mut flow.boxes {
        let Some(p) = ed.doc.paras.get(b.para) else { continue };
        if p.check_id.is_empty() {
            continue;
        }
        if let Some(&(dy, t0, dur)) = ed.view.slides.get(&p.check_id) {
            let t = ((now - t0) / dur).clamp(0.0, 1.0) as f32;
            let ease = 1.0 - (1.0 - t).powi(3);
            b.top += dy * (1.0 - ease);
        }
    }
    if !ed.view.slides.is_empty() {
        ed.request_repaint = true;
    }
}

fn mouse(ed: &mut Editor, ui: &mut egui::Ui, resp: &egui::Response) {
    let ctx = ui.ctx().clone();
    let Some(p) = ctx.input(|i| i.pointer.interact_pos()) else { return };
    let (shift, time) = ctx.input(|i| (i.modifiers.shift, i.time));
    // the paragraph under the mouse
    ed.view.hover = if resp.hovered() {
        ed.view.flow.as_ref().and_then(|f| f.boxes.iter().find(|b| !b.hidden && p.y >= ed.view.origin.y + b.top && p.y < ed.view.origin.y + b.bottom()).map(|b| b.para))
    } else {
        None
    };
    if resp.hovered() {
        let over_text = super::paint::link_at(ed, p).is_none();
        if over_text && ed.view.drag.is_none() {
            ctx.set_cursor_icon(egui::CursorIcon::Text);
        }
    }
    let pressed = ctx.input(|i| i.pointer.primary_pressed()) && resp.hovered();
    if pressed && !matches!(ed.view.drag, Some(Drag::Column(..))) {
        resp.request_focus();
        ed.resp.focus = true;
        ed.focused = true;
        ed.view.complete = None;
        // double and triple clicks
        let (t0, p0) = ed.view.last_click;
        ed.view.clicks = if time - t0 < 0.4 && (p - p0).length() < 5.0 { ed.view.clicks + 1 } else { 1 };
        ed.view.last_click = (time, p);
        ed.view.blink = time;
        // things that aren't text: a PDF's header, thumbnails, Quick Look
        if super::player::press(ed, p) {
            ed.view.drag = None;
            return;
        }
        if super::pdf::press(ed, p, ed.view.clicks) {
            ed.view.drag = None;
            return;
        }
        if let Some(i) = super::paint::grip_at(ed, p) {
            ed.view.drag = Some(Drag::Item(i));
            return;
        }
        if super::paint::press_widgets(ed, p) {
            ed.view.drag = None;
            return;
        }
        if let Some(sel) = super::table::press_handles(ed, p) {
            ed.table_sel = Some(sel);
            // (dragged: the row or column moves with the pointer)
            ed.view.drag = super::table::handle_at(ed, p).map(|(para, r, c)| match (r, c) {
                (Some(r), _) => Drag::Reorder(para, true, r),
                (_, c) => Drag::Reorder(para, false, c.unwrap_or(0)),
            });
            ed.view.reorder_undo = false;
            return;
        }
        let Some(l) = ed.hit(p, true) else { return };
        // a click on a drawing is drawn on (on one of its shapes: that shape, picked up); a
        // click on a picture, PDF or card selects it (drag it to move it)
        if l.cell.is_none() && !shift {
            if let Some(r) = ed.block_rect(l.para).filter(|r| r.contains(p)) {
                if ed.click_drawing(l.para, p, r) {
                    ed.view.drag = None;
                    ed.table_sel = None;
                    return;
                }
                if ed.doc.paras[l.para].runs.iter().filter_map(|r| r.fmt.att.as_deref()).any(|id| matches!(ed.doc.att(id).map(|a| &a.kind), Some(AttKind::Missing { .. }))) {
                    ed.alert = Some(("This attachment hasn’t been downloaded.".into(), "It’s still in iCloud. Open this note in Notes on a device signed in to iCloud to download it.".into()));
                }
                let n = ed.doc.paras[l.para].len();
                let sel = (Loc { cell: None, para: l.para, off: 0 }, Loc { cell: None, para: l.para, off: n });
                if (ed.anchor, ed.head) == sel || (ed.head, ed.anchor) == sel {
                    ed.view.drag = Some(Drag::Move);
                    if ed.view.clicks >= 2 {
                        ed.open_block(l.para);
                    }
                } else {
                    ed.anchor = sel.0;
                    ed.head = sel.1;
                    ed.view.drag = Some(Drag::Move);
                }
                ed.table_sel = None;
                return;
            }
        }
        match ed.view.clicks {
            1 => {
                // pressing inside the selection starts moving it
                if ed.has_sel() && !shift && ed.in_selection(p) {
                    ed.view.drag = Some(Drag::Move);
                    return;
                }
                ed.set_caret(l, shift);
                ed.view.drag = Some(match l.cell {
                    Some(c) => Drag::Cells(c),
                    None => Drag::Select,
                });
            }
            2 => {
                let t = &ed.para(l).text;
                let (a, b) = ops::word_at(t, l.off);
                ed.anchor = Loc { off: a, ..l };
                ed.head = Loc { off: b, ..l };
                ed.view.drag = Some(Drag::Words(ed.anchor, ed.head));
            }
            _ => {
                let n = ed.para(l).len();
                ed.anchor = Loc { off: 0, ..l };
                ed.head = Loc { off: n, ..l };
                ed.view.drag = Some(Drag::Words(ed.anchor, ed.head));
            }
        }
        ed.table_sel = None;
        return;
    }
    if resp.dragged() {
        match ed.view.drag.clone() {
            Some(Drag::Select) => {
                if let Some(l) = ed.hit(p, false) {
                    ed.head = l;
                }
                autoscroll(ed, p);
            }
            Some(Drag::Cells(c)) => {
                match ed.table_at(p) {
                    Some((para, Some(r), Some(col), _)) if para == c.para && (r != c.row || col != c.col) => {
                        ed.table_sel = Some(TableSel { para, r0: c.row.min(r), c0: c.col.min(col), r1: c.row.max(r), c1: c.col.max(col) });
                    }
                    Some((para, Some(r), Some(col), _)) if para == c.para && r == c.row && col == c.col => {
                        ed.table_sel = None;
                        let flow = ed.view.flow.as_ref().unwrap();
                        if let Some((f, o)) = ed.flow_geo(flow, ed.view.origin, Some(c)) {
                            ed.head = ed.hit_flow(f, o, p, Some(c), false);
                        }
                    }
                    _ => {}
                }
            }
            Some(Drag::Words(a, b)) => {
                if let Some(l) = ed.hit(p, false) {
                    if l.cell == a.cell {
                        let t = &ed.para(l).text;
                        let (wa, wb) = if ed.view.clicks >= 3 { (0, t.len()) } else { ops::word_at(t, l.off) };
                        if (l.para, l.off) < (a.para, a.off) {
                            ed.anchor = b;
                            ed.head = Loc { off: wa, ..l };
                        } else {
                            ed.anchor = a;
                            ed.head = Loc { off: wb, ..l };
                        }
                    }
                }
                autoscroll(ed, p);
            }
            Some(Drag::Move) => ctx.set_cursor_icon(egui::CursorIcon::Grabbing),
            Some(Drag::Reorder(para, row, at)) => {
                ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
                if let Some(to) = super::table::reorder_target(ed, para, row, at, p) {
                    if !ed.view.reorder_undo {
                        ed.push_undo(Kind::Other);
                        ed.view.reorder_undo = true;
                    }
                    ed.table_move(para, row, at, to);
                    ed.view.drag = Some(Drag::Reorder(para, row, to));
                }
                autoscroll(ed, p);
            }
            Some(Drag::Item(_)) => {
                ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
                // the gap nearest the pointer
                ed.view.drop_at = ed.view.flow.as_ref().map(|f| {
                    let y = p.y - ed.view.origin.y;
                    f.boxes.iter().filter(|b| !b.hidden).find(|b| y < b.top + b.height / 2.0).map_or(f.boxes.len(), |b| b.para)
                });
                autoscroll(ed, p);
            }
            _ => {}
        }
    }
    if resp.drag_stopped() || ctx.input(|i| i.pointer.primary_released()) {
        if let Some(Drag::Item(from)) = ed.view.drag {
            if let Some(to) = ed.view.drop_at.take() {
                ed.move_para(from, to);
            }
        }
        if ed.view.drag == Some(Drag::Move) {
            if resp.drag_stopped() {
                ed.move_selection_to(p, ctx.input(|i| i.modifiers.alt));
            } else if let Some(l) = ed.hit(p, true) {
                // a click in the selection without dragging puts the caret there
                ed.set_caret(l, false);
            }
        }
        ed.view.drag = None;
    }
    if resp.clicked() && ed.view.clicks == 1 && !shift && !ed.has_sel() {
        // a click on a link, tag or note link opens it
        if let Some(what) = super::paint::link_at(ed, p) {
            match what {
                super::paint::Link::Web(u) => ed.resp.open_link = Some(u),
                super::paint::Link::Note(u) => ed.resp.open_note = Some(u),
                super::paint::Link::Tag(t) => ed.resp.tag_clicked = Some(t),
            }
        }
    }
}

fn autoscroll(ed: &mut Editor, p: Pos2) {
    let Some(r) = ed.view.rect else { return };
    if p.y > r.bottom() - 10.0 {
        ed.view.scroll.offset += (p.y - (r.bottom() - 10.0)).min(30.0);
    } else if p.y < r.top() + 10.0 {
        ed.view.scroll.offset -= ((r.top() + 10.0) - p.y).min(30.0);
    }
}

impl Editor {
    /// The screen rectangle of a picture / PDF / card paragraph of the note.
    pub(crate) fn block_rect(&self, para: usize) -> Option<Rect> {
        let pb = self.view.flow.as_ref()?.boxes.get(para)?;
        let Some(Block::Media(size)) = &pb.block else { return None };
        Some(Rect::from_min_size(self.view.origin + vec2(pb.x, pb.top + layout::MEDIA_PAD * self.settings.text_scale), *size))
    }

    /// Double-click on an attachment: a drawing is drawn on, others open in their app (Quick
    /// Look on a Mac).
    fn open_block(&mut self, para: usize) {
        let Some(id) = self.doc.paras.get(para).and_then(|p| p.sole_att()) else { return };
        if matches!(self.doc.att(id).map(|a| &a.kind), Some(AttKind::Drawing { .. })) {
            self.head = Loc { cell: None, para, off: 0 };
            self.anchor = self.head;
            self.markup();
            self.changed = true;
            return;
        }
        let path = match self.doc.att(id).map(|a| &a.kind) {
            Some(AttKind::Image { path, .. } | AttKind::Drawing { path, .. } | AttKind::File { path, .. } | AttKind::Pdf { path, .. }) => path.clone(),
            Some(AttKind::Url { url, .. }) => {
                self.resp.open_link = Some(url.clone());
                None
            }
            _ => None,
        };
        if let Some(p) = path {
            super::open_external(&p);
        }
    }

    /// Files dropped on the note go in where they are dropped.
    fn drop_files(&mut self, ctx: &egui::Context) {
        let files: Vec<std::path::PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).filter(|p| p.exists()).collect());
        if files.is_empty() {
            return;
        }
        let at = ctx.input(|i| i.pointer.hover_pos()).and_then(|p| self.hit(p, false));
        if let Some(l) = at {
            self.set_caret(l, false);
        }
        for f in files {
            self.attach_file(&f);
        }
        self.resp.focus = true;
    }

    /// The point is over selected text (or a selected picture).
    fn in_selection(&self, p: Pos2) -> bool {
        let (a, b) = self.range();
        if a.cell.is_none() && a.para == b.para && a.off == 0 && b.off == self.doc.paras[a.para].len() && self.block_rect(a.para).is_some_and(|r| r.contains(p)) {
            return true;
        }
        let Some(l) = self.hit(p, true) else { return false };
        let (a, b) = self.range();
        l.cell == a.cell && (l.para, l.off) > (a.para, a.off) && (l.para, l.off) < (b.para, b.off)
    }

    /// Drops dragged text at a point (moved, or copied with Alt).
    fn move_selection_to(&mut self, p: Pos2, copy: bool) {
        let Some(to) = self.hit(p, true) else { return };
        let (a, b) = self.range();
        if to.cell == a.cell && (to.para, to.off) >= (a.para, a.off) && (to.para, to.off) <= (b.para, b.off) {
            self.set_caret(to, false);
            return;
        }
        // a picture or card moves as its paragraph
        let whole_block = a.cell.is_none() && a.para == b.para && a.off == 0 && b.off == self.doc.paras[a.para].len() && self.doc.block_of(&self.doc.paras[a.para]).is_some();
        if whole_block {
            if to.cell.is_some() {
                return;
            }
            self.push_undo(Kind::Other);
            let mut p = if copy { self.doc.paras[a.para].clone() } else { self.doc.paras.remove(a.para) };
            if copy {
                if let Some(att) = p.sole_att().and_then(|id| self.doc.att(id)).cloned() {
                    let mut att = att;
                    att.id = super::doc::new_uuid();
                    att.model = None;
                    p.set(0..p.len(), &|f: &mut super::doc::Fmt| f.att = Some(att.id.clone()));
                    self.doc.atts.push(att);
                }
            }
            let mut at = to.para + usize::from(to.off > 0);
            if !copy && at > a.para {
                at -= 1;
            }
            let at = at.min(self.doc.paras.len());
            let n = p.len();
            self.doc.paras.insert(at, p);
            self.anchor = Loc { cell: None, para: at, off: 0 };
            self.head = Loc { cell: None, para: at, off: n };
            self.changed = true;
            return;
        }
        // (into a table that's part of what moves: nowhere)
        if !copy && a.cell.is_none() && to.cell.is_some_and(|c| c.para >= a.para && c.para <= b.para) {
            return;
        }
        let Some((frag, _)) = self.copy_frag() else { return };
        self.push_undo(Kind::Other);
        let mut to = to;
        if !copy {
            // removing the text first moves a drop point after it
            let same = to.cell == a.cell;
            let after = same && (to.para, to.off) > (b.para, b.off);
            // (a table cell below the body text moved: the table is that many paragraphs up)
            if a.cell.is_none() {
                if let Some(c) = to.cell.as_mut().filter(|c| c.para > b.para) {
                    c.para -= b.para - a.para;
                }
            }
            self.delete_sel();
            if after {
                if to.para == b.para {
                    to.off = to.off - b.off + a.off;
                    to.para = a.para;
                } else {
                    to.para -= b.para - a.para;
                }
            }
        }
        self.anchor = to;
        self.head = to;
        let undo_len = self.undo.len();
        self.paste_frag(&frag);
        // one undo step for the whole move
        self.undo.truncate(undo_len);
        let end = self.head;
        self.anchor = to;
        self.head = end;
    }
}
