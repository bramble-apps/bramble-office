//! Editing operations on the working copy: typing (with Notes' auto-formatting), Return and
//! Delete with the list rules, paragraph and character styles, checklists, moving items,
//! collapsing sections, links, tags, dividers, clipboard fragments and undo. Nothing here draws,
//! so the unit tests drive it directly.

use nt_core::model::ParaStyle;

use super::doc::{Align, Att, AttKind, Doc, Fmt, OBJ, Para, Table, new_uuid};
use super::{Cell, Editor, Loc};

/// What kind of change an undo step is (consecutive typing merges into one step).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Typing,
    Delete,
    Other,
}

#[derive(Clone, Debug)]
pub struct Snap {
    pub doc: Doc,
    pub anchor: Loc,
    pub head: Loc,
}

/// Copied text with its formatting and attachments (pasted with its formatting inside the app).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Frag {
    pub paras: Vec<Para>,
    pub atts: Vec<Att>,
    /// it ends inside a paragraph (the last paragraph is partial)
    pub open_end: bool,
}

pub fn prev_char(t: &str, i: usize) -> usize {
    t[..i].char_indices().next_back().map_or(0, |(j, _)| j)
}

pub fn next_char(t: &str, i: usize) -> usize {
    t[i..].chars().next().map_or(t.len(), |c| i + c.len_utf8())
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '\'' || c == '\u{2019}'
}

pub fn prev_word(t: &str, i: usize) -> usize {
    let mut j = i;
    while j > 0 && !is_word(t[..j].chars().next_back().unwrap()) {
        j = prev_char(t, j);
    }
    while j > 0 && is_word(t[..j].chars().next_back().unwrap()) {
        j = prev_char(t, j);
    }
    j
}

pub fn next_word(t: &str, i: usize) -> usize {
    let mut j = i;
    while j < t.len() && !is_word(t[j..].chars().next().unwrap()) {
        j = next_char(t, j);
    }
    while j < t.len() && is_word(t[j..].chars().next().unwrap()) {
        j = next_char(t, j);
    }
    j
}

/// The word around `i` (double-click).
pub fn word_at(t: &str, i: usize) -> (usize, usize) {
    let at_word = t[i..].chars().next().is_some_and(is_word) || (i > 0 && t[..i].chars().next_back().is_some_and(is_word));
    if !at_word {
        return (i, next_char(t, i));
    }
    let mut a = i;
    while a > 0 && is_word(t[..a].chars().next_back().unwrap()) {
        a = prev_char(t, a);
    }
    let mut b = i;
    while b < t.len() && is_word(t[b..].chars().next().unwrap()) {
        b = next_char(t, b);
    }
    (a, b)
}

impl Editor {
    // ---- flows and positions

    pub fn flow(&self, cell: Option<Cell>) -> &Vec<Para> {
        match cell {
            None => &self.doc.paras,
            Some(c) => {
                let id = self.doc.paras.get(c.para).and_then(|p| p.sole_att()).unwrap_or_default();
                match self.doc.table(id) {
                    Some(t) if c.row < t.rows() && c.col < t.cols() => &t.cells[c.row][c.col],
                    _ => &self.doc.paras,
                }
            }
        }
    }

    pub fn flow_mut(&mut self, cell: Option<Cell>) -> &mut Vec<Para> {
        if let Some(c) = cell {
            let id = self.doc.paras.get(c.para).and_then(|p| p.sole_att()).unwrap_or_default().to_string();
            let ok = self.doc.table(&id).is_some_and(|t| c.row < t.rows() && c.col < t.cols());
            if ok {
                return &mut self.doc.table_mut(&id).unwrap().cells[c.row][c.col];
            }
        }
        &mut self.doc.paras
    }

    pub fn para(&self, l: Loc) -> &Para {
        let f = self.flow(l.cell);
        &f[l.para.min(f.len() - 1)]
    }

    /// Keeps a position inside its flow, on a character boundary.
    pub fn clamp(&self, mut l: Loc) -> Loc {
        if let Some(c) = l.cell {
            let ok = self.doc.paras.get(c.para).and_then(|p| p.sole_att()).and_then(|id| self.doc.table(id)).is_some_and(|t| c.row < t.rows() && c.col < t.cols());
            if !ok {
                l = Loc { cell: None, para: c.para, off: 0 };
            }
        }
        let f = self.flow(l.cell);
        l.para = l.para.min(f.len().saturating_sub(1));
        let t = &f[l.para].text;
        l.off = l.off.min(t.len());
        while !t.is_char_boundary(l.off) {
            l.off -= 1;
        }
        l
    }

    pub fn has_sel(&self) -> bool {
        self.anchor != self.head
    }

    /// The selection, start first.
    pub fn range(&self) -> (Loc, Loc) {
        let (a, b) = (self.anchor, self.head);
        if (a.para, a.off) <= (b.para, b.off) { (a, b) } else { (b, a) }
    }

    pub fn set_caret(&mut self, l: Loc, extend: bool) {
        let l = self.clamp(l);
        if extend && l.cell == self.anchor.cell {
            self.head = l;
        } else {
            self.anchor = l;
            self.head = l;
        }
        self.pending = None;
        self.goal_x = None;
        self.break_undo = true;
        self.table_sel = None;
    }

    /// The paragraphs the selection touches (in its flow).
    /// Format > Alignment > Writing Direction for the selected paragraphs (an undo step).
    pub fn set_direction(&mut self, d: u32) {
        if self.head.cell.is_some() {
            return;
        }
        self.push_undo(Kind::Other);
        for i in self.sel_paras() {
            self.doc.paras[i].direction = Some(d);
        }
        self.changed = true;
    }

    pub fn sel_paras(&self) -> std::ops::RangeInclusive<usize> {
        let (a, b) = self.range();
        // a selection ending at the start of a paragraph doesn't include it
        let end = if b.para > a.para && b.off == 0 { b.para - 1 } else { b.para };
        a.para..=end
    }

    // ---- undo

    pub fn push_undo(&mut self, kind: Kind) {
        if kind == Kind::Typing && self.last_kind == Some(Kind::Typing) && !self.break_undo {
            return;
        }
        self.undo.push(Snap { doc: self.doc.clone(), anchor: self.anchor, head: self.head });
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.last_kind = Some(kind);
        self.break_undo = false;
    }

    pub fn undo(&mut self) {
        // while drawing, undo is the drawing's
        if self.draw.is_some() {
            self.draw_undo(false);
            return;
        }
        if let Some(s) = self.undo.pop() {
            self.redo.push(Snap { doc: std::mem::replace(&mut self.doc, s.doc), anchor: self.anchor, head: self.head });
            self.anchor = self.clamp(s.anchor);
            self.head = self.clamp(s.head);
            self.changed = true;
            self.break_undo = true;
        }
    }

    pub fn redo(&mut self) {
        if self.draw.is_some() {
            self.draw_undo(true);
            return;
        }
        if let Some(s) = self.redo.pop() {
            self.undo.push(Snap { doc: std::mem::replace(&mut self.doc, s.doc), anchor: self.anchor, head: self.head });
            self.anchor = self.clamp(s.anchor);
            self.head = self.clamp(s.head);
            self.changed = true;
            self.break_undo = true;
        }
    }

    // ---- text

    /// Removes the selected text; the caret ends at its start.
    pub fn delete_sel(&mut self) -> bool {
        if !self.has_sel() {
            return false;
        }
        let (a, b) = self.range();
        let flow = self.flow_mut(a.cell);
        if a.para == b.para {
            flow[a.para].remove(a.off..b.off);
        } else {
            let tail = flow[b.para].slice(b.off..flow[b.para].len());
            let n = flow[a.para].len();
            flow[a.para].remove(a.off..n);
            // a block (table, picture) whose paragraph goes entirely goes with it
            flow[a.para].append(&tail);
            flow.drain(a.para + 1..=b.para);
        }
        self.anchor = a;
        self.head = a;
        self.changed = true;
        true
    }

    /// The format typing at the caret gets.
    pub fn typing_fmt(&self) -> Fmt {
        self.pending.clone().unwrap_or_else(|| {
            let p = self.para(self.head);
            let off = self.head.off;
            let mut f = p.fmt_at(off);
            // inside a link the typing is part of it (at its end it isn't)
            let before = if off == 0 { None } else { p.fmt_of(prev_char(&p.text, off)).and_then(|x| x.link.clone()) };
            let after = p.fmt_of(off).and_then(|x| x.link.clone());
            if before.is_some() && before == after {
                f.link = before;
            }
            f
        })
    }

    /// Inserts text (newlines make paragraphs like the current one) at the caret.
    pub fn insert_text(&mut self, s: &str, fmt: &Fmt) {
        self.delete_sel();
        self.leave_block();
        let mut l = self.head;
        for (i, line) in s.split('\n').enumerate() {
            if i > 0 {
                self.split_para();
                l = self.head;
            }
            let flow = self.flow_mut(l.cell);
            flow[l.para].insert(l.off, line, fmt);
            l.off += line.len();
            self.head = l;
            self.anchor = l;
        }
        self.changed = true;
    }

    /// The caret is on a block attachment's line: makes an empty line before or after it to type in.
    fn leave_block(&mut self) {
        let l = self.head;
        if l.cell.is_some() || self.doc.block_of(&self.doc.paras[l.para]).is_none() {
            return;
        }
        let p = Para { style: ParaStyle::Body, ..Default::default() };
        if l.off == 0 {
            self.doc.paras.insert(l.para, p);
            self.head = Loc { cell: None, para: l.para, off: 0 };
        } else {
            self.doc.paras.insert(l.para + 1, p);
            self.head = Loc { cell: None, para: l.para + 1, off: 0 };
        }
        self.anchor = self.head;
    }

    /// Splits the paragraph at the caret (no list rules).
    fn split_para(&mut self) {
        let l = self.head;
        let flow = self.flow_mut(l.cell);
        let mut rest = flow[l.para].split_off(l.off);
        if rest.is_heading() && rest.is_empty() {
            rest.style = ParaStyle::Body;
        }
        rest.checked = false;
        flow.insert(l.para + 1, rest);
        self.head = Loc { cell: l.cell, para: l.para + 1, off: 0 };
        self.anchor = self.head;
    }

    /// Typing: Notes' smart quotes and dashes, lists from "- ", "* ", "• ", "1. ", "1) ", tags
    /// from "#word ", links from "www.… ", text replacements, then the text.
    pub fn type_text(&mut self, t: &str) {
        if t.is_empty() {
            return;
        }
        if self.table_sel.is_some() {
            self.table_sel = None;
        }
        self.push_undo(Kind::Typing);
        let had_sel = self.has_sel();
        self.delete_sel();
        self.leave_block();
        let fmt = self.typing_fmt();
        let l = self.head;
        let before: String = self.para(l).text[..l.off].to_string();
        let prev = before.chars().next_back();
        let mut ins = t.to_string();
        let mut back = 0;
        if self.settings.smart_dashes && t == "-" && prev == Some('\u{2014}') && !had_sel {
            // "---" stays one em dash
            ins.clear();
        } else {
            let (b, s) = kn_ui::typing::smart_text(prev, t, !had_sel, self.settings.smart_quotes, self.settings.smart_dashes);
            back = b;
            ins = s;
        }
        let word_end = t.chars().all(|c| c.is_whitespace() || ".,;:!?)".contains(c));
        // (a full stop or a colon may be inside an address: links wait for a space or a comma…)
        let link_end = t.chars().all(|c| c.is_whitespace() || ",;!?)".contains(c));
        let mut linked = None;
        if word_end {
            if t == " " && self.auto_list(&before) {
                return;
            }
            if self.settings.smart_tags && self.auto_tag(&before) {
                self.insert_after_token(t, &fmt);
                return;
            }
            if self.settings.smart_links && link_end {
                linked = self.auto_link(&before);
            }
            let l = self.head;
            let before: String = self.para(l).text[..l.off].to_string();
            // Correct Spelling Automatically: a misspelled word becomes the first suggestion
            if self.settings.autocorrect && !before.ends_with(OBJ) && self.para(l).fmt_of(before.len().saturating_sub(1)).is_none_or(|f| f.link.is_none()) {
                if let Some((from, fixed)) = kn_ui::typing::word_fix(&before, false, true) {
                    let flow = self.flow_mut(l.cell);
                    let f = flow[l.para].fmt_at(from + 1);
                    flow[l.para].remove(from..l.off);
                    flow[l.para].insert(from, &fixed, &f);
                    self.head = Loc { off: from + fixed.len(), ..l };
                    self.anchor = self.head;
                }
            }
            let l = self.head;
            let before: String = self.para(l).text[..l.off].to_string();
            if self.settings.text_replacement {
                if let Some((from, to)) = replacement(&before) {
                    let flow = self.flow_mut(l.cell);
                    let f = flow[l.para].fmt_at(from);
                    flow[l.para].remove(from..l.off);
                    flow[l.para].insert(from, to, &f);
                    let off = from + to.len();
                    self.head = Loc { off, ..l };
                    self.anchor = self.head;
                }
            }
        }
        let l = self.head;
        let mut from = l.off;
        for _ in 0..back {
            from = prev_char(&self.para(l).text, from);
        }
        let flow = self.flow_mut(l.cell);
        flow[l.para].remove(from..l.off);
        // (inside a link the typing is part of it; typing_fmt already left it out at its end)
        let fmt = Fmt { link: fmt.link.clone(), ..fmt.typing() };
        flow[l.para].insert(from, &ins, &fmt);
        self.head = Loc { off: from + ins.len(), ..l };
        self.anchor = self.head;
        self.changed = true;
        if let Some(at) = linked {
            self.undo_step_without_link(at);
        } else {
            // typed inside a link that is its address: it follows
            self.refresh_links(l.cell, l.para);
        }
    }

    fn insert_after_token(&mut self, t: &str, fmt: &Fmt) {
        let l = self.head;
        let flow = self.flow_mut(l.cell);
        flow[l.para].insert(l.off, t, &fmt.typing());
        self.head = Loc { off: l.off + t.len(), ..l };
        self.anchor = self.head;
        self.changed = true;
    }

    /// "- ", "* ", "• ", "1. ", "1) " at the start of a body paragraph start a list.
    fn auto_list(&mut self, before: &str) -> bool {
        if !self.settings.smart_lists {
            return false;
        }
        let l = self.head;
        let p = self.para(l);
        if p.style != ParaStyle::Body || p.quote {
            return false;
        }
        let (style, start) = match before {
            "-" => (ParaStyle::Dashed, None),
            "*" | "\u{2022}" => (ParaStyle::Bulleted, None),
            _ => {
                let num = before.strip_suffix('.').or_else(|| before.strip_suffix(')'));
                match num.and_then(|n| n.parse::<u32>().ok()).filter(|_| num.is_some_and(|n| !n.is_empty() && n.len() < 6)) {
                    Some(n) => (ParaStyle::Numbered, Some(n)),
                    None => return false,
                }
            }
        };
        let flow = self.flow_mut(l.cell);
        let p = &mut flow[l.para];
        p.remove(0..l.off);
        p.style = style;
        p.start = start;
        self.head = Loc { off: 0, ..l };
        self.anchor = self.head;
        self.changed = true;
        true
    }

    /// "#word" before the caret becomes a tag.
    fn auto_tag(&mut self, before: &str) -> bool {
        let start = before.rfind(|c: char| c.is_whitespace()).map_or(0, |i| i + before[i..].chars().next().unwrap().len_utf8());
        let w = &before[start..];
        let Some(name) = w.strip_prefix('#') else { return false };
        if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') || !name.chars().any(|c| c.is_alphabetic()) {
            return false;
        }
        let l = self.head;
        if self.para(l).text[start..l.off].contains(OBJ) {
            return false;
        }
        self.insert_token(start..l.off, AttKind::Tag(w.to_string()), "com.apple.notes.inlinetextattachment.hashtag");
        true
    }

    /// Replaces `r` of the caret's paragraph with an inline token; the caret goes after it.
    pub fn insert_token(&mut self, r: std::ops::Range<usize>, kind: AttKind, uti: &str) {
        let id = new_uuid();
        self.doc.atts.push(Att { id: id.clone(), uti: uti.into(), kind, token: None, model: None });
        let l = self.head;
        let flow = self.flow_mut(l.cell);
        let base = flow[l.para].fmt_at(r.start).typing();
        flow[l.para].remove(r.clone());
        flow[l.para].insert(r.start, "\u{FFFC}", &Fmt { att: Some(id), ..base });
        self.head = Loc { off: r.start + OBJ.len_utf8(), ..l };
        self.anchor = self.head;
        self.changed = true;
    }

    /// Smart Links (Notes' Edit > Substitutions): the address just typed before the caret —
    /// "https://…", "www.example.org", "example.com", "name@example.org" — becomes a link when a
    /// space, Return or punctuation follows it. Undo straight after takes back only the link.
    /// True when it linked.
    fn auto_link(&mut self, before: &str) -> Option<(Option<super::Cell>, usize, std::ops::Range<usize>)> {
        let start = before.rfind(|c: char| c.is_whitespace() || c == OBJ).map_or(0, |i| i + before[i..].chars().next().unwrap().len_utf8());
        let w = before[start..].trim_end_matches(['.', ',', ')', ';', ':', '!', '?', '"', '\'', '\u{201D}', '\u{2019}']);
        let w = w.trim_start_matches(['(', '"', '\'', '\u{201C}', '\u{2018}']);
        let start = before.len() - before[start..].len() + before[start..].find(w).unwrap_or(0);
        let url = link_target(w)?;
        let l = self.head;
        // already linked: left alone, unless it is an address link short of the whole word
        if let Some(f) = self.para(l).fmt_of(start) {
            if f.att.is_some() || f.link.as_ref().is_some_and(|k| *k == url || !was_address(k)) {
                return None;
            }
        }
        let flow = self.flow_mut(l.cell);
        flow[l.para].set(start..start + w.len(), &|f: &mut Fmt| f.link = Some(url.clone()));
        self.changed = true;
        Some((l.cell, l.para, start..start + w.len()))
    }

    /// After a keystroke that made a link: an undo step that is the note as it is now but for the
    /// link, so Undo takes back only the link.
    fn undo_step_without_link(&mut self, at: (Option<super::Cell>, usize, std::ops::Range<usize>)) {
        let (cell, para, r) = at;
        let kept = self.doc.clone();
        if let Some(p) = self.flow_mut(cell).get_mut(para) {
            if r.end <= p.len() {
                p.set(r, &|f: &mut Fmt| f.link = None);
            }
        }
        let unlinked = std::mem::replace(&mut self.doc, kept);
        self.undo.push(Snap { doc: unlinked, anchor: self.anchor, head: self.head });
        self.redo.clear();
        self.last_kind = Some(Kind::Other);
        self.break_undo = true;
    }

    /// A link whose text is an address follows its text as it is edited ("www.apple.com" made
    /// "www.apple.co" links there); links with other text are left as they are.
    pub fn refresh_links(&mut self, cell: Option<super::Cell>, para: usize) {
        let Some(p) = self.flow(cell).get(para) else { return };
        let mut fixes: Vec<(std::ops::Range<usize>, String)> = Vec::new();
        let mut i = 0;
        let spans: Vec<(std::ops::Range<usize>, Option<String>)> = p.runs.iter().map(|r| {
            let a = i;
            i += r.len;
            (a..i, r.fmt.link.clone())
        }).collect();
        let mut k = 0;
        while k < spans.len() {
            let Some(link) = spans[k].1.clone() else {
                k += 1;
                continue;
            };
            let mut range = spans[k].0.clone();
            while k + 1 < spans.len() && spans[k + 1].1.as_deref() == Some(link.as_str()) {
                k += 1;
                range.end = spans[k].0.end;
            }
            if let Some(t) = link_target(&p.text[range.clone()]) {
                if t != link && was_address(&link) {
                    fixes.push((range, t));
                }
            }
            k += 1;
        }
        for (r, t) in fixes {
            let flow = self.flow_mut(cell);
            flow[para].set(r, &|f: &mut Fmt| f.link = Some(t.clone()));
            self.changed = true;
        }
    }

    /// Return: a new paragraph; an empty list item ends the list (or goes up a level), an empty
    /// quote line ends the quote; after a heading comes Body.
    pub fn new_paragraph(&mut self) {
        self.push_undo(Kind::Other);
        // Return after an address links it too (Smart Links)
        let linked = if self.settings.smart_links && !self.has_sel() {
            let before: String = self.para(self.head).text[..self.head.off].to_string();
            self.auto_link(&before)
        } else {
            None
        };
        self.new_paragraph_inner();
        if let Some(at) = linked {
            self.undo_step_without_link(at);
        }
    }

    fn new_paragraph_inner(&mut self) {
        self.delete_sel();
        let l = self.head;
        if l.cell.is_none() && self.doc.block_of(&self.doc.paras[l.para]).is_some() {
            // before a picture: an empty line above it, the caret stays before it; after: a line to type in
            self.doc.paras.insert(if l.off == 0 { l.para } else { l.para + 1 }, Para::default());
            self.head = Loc { cell: None, para: l.para + 1, off: 0 };
            self.anchor = self.head;
            self.changed = true;
            return;
        }
        let p = self.para(l).clone();
        if p.is_empty() && (p.is_list() || p.quote) {
            let flow = self.flow_mut(l.cell);
            let q = &mut flow[l.para];
            if q.is_list() && q.indent > 0 {
                q.indent -= 1;
            } else if q.is_list() {
                q.style = ParaStyle::Body;
                q.checked = false;
                q.start = None;
            } else {
                q.quote = false;
            }
            self.changed = true;
            return;
        }
        let flow = self.flow_mut(l.cell);
        let mut rest = flow[l.para].split_off(l.off);
        if rest.is_heading() && rest.is_empty() {
            rest.style = ParaStyle::Body;
        }
        rest.checked = false;
        if l.off == 0 && !p.is_empty() {
            // Return at the start: the empty line above keeps the style, the text keeps its ticks
            rest.checked = p.checked;
            flow[l.para].checked = false;
        }
        flow.insert(l.para + 1, rest);
        self.head = Loc { cell: l.cell, para: l.para + 1, off: 0 };
        self.anchor = self.head;
        self.pending = None;
        self.changed = true;
    }

    /// Backspace (or with `word`, back to the word's start).
    pub fn backspace(&mut self, word: bool) {
        self.backspace_inner(word);
        // a link that is its address follows the edit
        let h = self.head;
        self.refresh_links(h.cell, h.para);
    }

    fn backspace_inner(&mut self, word: bool) {
        if self.table_sel.is_some() {
            self.clear_table_sel();
            return;
        }
        self.push_undo(Kind::Delete);
        if self.delete_sel() {
            return;
        }
        let l = self.head;
        // straight after an autocorrection: the word as typed comes back
        if l.off > 0 && !word {
            if let Some((n, typed)) = kn_ui::typing::take_back(&self.para(l).text[..l.off]) {
                let from = l.off - n;
                let flow = self.flow_mut(l.cell);
                let f = flow[l.para].fmt_at(from + 1);
                flow[l.para].remove(from..l.off);
                flow[l.para].insert(from, &typed, &f);
                self.head = Loc { off: from + typed.len(), ..l };
                self.anchor = self.head;
                self.changed = true;
                return;
            }
        }
        if l.off > 0 {
            let t = &self.para(l).text;
            let from = if word { prev_word(t, l.off) } else { prev_char(t, l.off) };
            let flow = self.flow_mut(l.cell);
            flow[l.para].remove(from..l.off);
            self.head = Loc { off: from, ..l };
            self.anchor = self.head;
            self.changed = true;
            return;
        }
        let p = self.para(l).clone();
        // (NO2) after another list item, Backspace at an item's start joins it to that item in one
        // step; the first item of a list goes back to Body first
        let after_item = l.para > 0 && self.flow(l.cell)[l.para - 1].is_list() && p.indent == 0;
        let flow = self.flow_mut(l.cell);
        if p.is_list() && !after_item {
            let q = &mut flow[l.para];
            if q.indent > 0 {
                q.indent -= 1;
            } else {
                q.style = ParaStyle::Body;
                q.checked = false;
                q.start = None;
            }
        } else if p.quote {
            flow[l.para].quote = false;
        } else if p.indent > 0 {
            flow[l.para].indent -= 1;
        } else if l.para > 0 {
            let prev_block = l.cell.is_none() && self.doc.block_of(&self.doc.paras[l.para - 1]).is_some();
            let flow = self.flow_mut(l.cell);
            if prev_block || flow[l.para - 1].is_empty() {
                // the picture or table before goes; an empty line before goes and this one keeps its style
                flow.remove(l.para - 1);
                self.head = Loc { para: l.para - 1, off: 0, ..l };
                if prev_block && !p.is_empty() {
                    self.head.off = 0;
                }
            } else {
                let n = flow[l.para - 1].len();
                let cur = flow.remove(l.para);
                flow[l.para - 1].append(&cur);
                self.head = Loc { para: l.para - 1, off: n, ..l };
            }
        } else {
            return;
        }
        self.anchor = self.head;
        self.changed = true;
    }

    /// Delete forward.
    pub fn delete_forward(&mut self, word: bool) {
        self.delete_forward_inner(word);
        // a link that is its address follows the edit
        let h = self.head;
        self.refresh_links(h.cell, h.para);
    }

    fn delete_forward_inner(&mut self, word: bool) {
        self.push_undo(Kind::Delete);
        if self.delete_sel() {
            return;
        }
        let l = self.head;
        let len = self.para(l).len();
        if l.off < len {
            let t = &self.para(l).text;
            let to = if word { next_word(t, l.off) } else { next_char(t, l.off) };
            let flow = self.flow_mut(l.cell);
            flow[l.para].remove(l.off..to);
        } else {
            let next_block = l.cell.is_none() && self.doc.paras.get(l.para + 1).is_some_and(|p| self.doc.block_of(p).is_some());
            let flow = self.flow_mut(l.cell);
            if l.para + 1 >= flow.len() {
                return;
            }
            if next_block {
                flow.remove(l.para + 1);
            } else {
                let next = flow.remove(l.para + 1);
                flow[l.para].append(&next);
            }
        }
        self.changed = true;
    }

    // ---- paragraph styles

    fn each_para(&mut self, f: impl Fn(&mut Para)) {
        let r = self.sel_paras();
        let cell = self.head.cell;
        let blocks: Vec<bool> = self.flow(cell).iter().map(|p| cell.is_none() && self.doc.block_of(p).is_some()).collect();
        let flow = self.flow_mut(cell);
        for i in r {
            if !blocks[i] {
                f(&mut flow[i]);
            }
        }
        self.changed = true;
    }

    /// Format > Title / Heading / … ; choosing a list or checklist again turns it back to Body.
    pub fn set_style(&mut self, style: ParaStyle) {
        self.push_undo(Kind::Other);
        let r = self.sel_paras();
        let all = self.flow(self.head.cell)[r].iter().all(|p| p.style == style);
        let toggle_off = all && matches!(style, ParaStyle::Bulleted | ParaStyle::Dashed | ParaStyle::Numbered | ParaStyle::Checklist);
        self.each_para(|p| {
            if toggle_off {
                p.style = ParaStyle::Body;
            } else {
                if p.style != style {
                    p.checked = false;
                }
                p.style = style;
            }
            if p.style != ParaStyle::Numbered {
                p.start = None;
            }
        });
    }

    pub fn toggle_quote(&mut self) {
        self.push_undo(Kind::Other);
        let r = self.sel_paras();
        let all = self.flow(self.head.cell)[r].iter().all(|p| p.quote);
        self.each_para(|p| p.quote = !all);
    }

    pub fn indent(&mut self, by: i32) {
        self.push_undo(Kind::Other);
        self.each_para(|p| p.indent = (p.indent as i32 + by).clamp(0, 12) as u8);
    }

    pub fn align(&mut self, a: Align) {
        self.push_undo(Kind::Other);
        self.each_para(|p| p.align = a);
    }

    // ---- checklists

    /// Ticks or unticks checklist item `i` of the body (the circle), sorting ticked items to the
    /// bottom of their list when Settings says so.
    /// Ticks or unticks it; returns the item's id (its checklist uuid). With Settings' sort on, a
    /// ticked item goes to the bottom of its list at once and an unticked one back up above the
    /// ticked ones (the window slides them there).
    pub fn set_checked(&mut self, cell: Option<Cell>, i: usize, on: bool) -> Option<String> {
        if self.flow(cell).get(i).is_none_or(|p| p.style != ParaStyle::Checklist) {
            return None;
        }
        self.push_undo(Kind::Other);
        let flow = self.flow_mut(cell);
        if flow[i].check_id.is_empty() {
            flow[i].check_id = new_uuid();
        }
        flow[i].checked = on;
        let id = flow[i].check_id.clone();
        self.changed = true;
        // a tick with "Automatically sort checked items" moves it; unticking leaves it (NO2)
        if self.settings.sort_checked && on {
            self.sort_checked_group(cell, i, true);
        }
        Some(id)
    }

    /// Moves body paragraph `from` (with the items nested under it) to sit before paragraph `to`.
    pub fn move_para(&mut self, from: usize, to: usize) {
        let n = self.doc.paras.len();
        if from >= n || to > n {
            return;
        }
        let lvl = self.doc.paras[from].indent;
        let mut end = from + 1;
        while end < n && self.doc.paras[end].is_list() && self.doc.paras[end].indent > lvl {
            end += 1;
        }
        if (from..=end).contains(&to) {
            return;
        }
        self.push_undo(Kind::Other);
        let moved: Vec<Para> = self.doc.paras.drain(from..end).collect();
        let at = if to > from { to - moved.len() } else { to };
        let k = moved.len();
        self.doc.paras.splice(at..at, moved);
        let l = Loc { cell: None, para: at + k - 1, off: self.doc.paras[at + k - 1].len() };
        self.anchor = l;
        self.head = l;
        self.changed = true;
    }

    /// Format > Mark as Checked: toggles the selected items (all ticked if any isn't).
    pub fn mark_checked(&mut self) {
        let r = self.sel_paras();
        let cell = self.head.cell;
        let items: Vec<usize> = r.filter(|&i| self.flow(cell)[i].style == ParaStyle::Checklist).collect();
        if items.is_empty() {
            return;
        }
        let on = !items.iter().all(|&i| self.flow(cell)[i].checked);
        self.push_undo(Kind::Other);
        for &i in &items {
            self.flow_mut(cell)[i].checked = on;
        }
        // (Notes doesn't re-sort for ⇧⌘U, NO2)
        self.changed = true;
    }

    /// The run of checklist items around `i`.
    fn checklist_group(&self, cell: Option<Cell>, i: usize) -> std::ops::Range<usize> {
        let f = self.flow(cell);
        let mut a = i;
        while a > 0 && f[a - 1].style == ParaStyle::Checklist {
            a -= 1;
        }
        let mut b = i;
        while b < f.len() && f[b].style == ParaStyle::Checklist {
            b += 1;
        }
        a..b
    }

    fn sort_checked_group(&mut self, cell: Option<Cell>, i: usize, trigger: bool) {
        let g = self.checklist_group(cell, i);
        let start = g.start;
        let flow = self.flow_mut(cell);
        // every item gets its uuid (Notes' own), so the window can follow it as it moves
        for p in &mut flow[g.clone()] {
            if p.check_id.is_empty() {
                p.check_id = new_uuid();
            }
        }
        let flow = self.flow(cell);
        let caret_id = (self.head.cell == cell && g.contains(&self.head.para)).then(|| flow[self.head.para].check_id.clone());
        let anchor_id = (self.anchor.cell == cell && g.contains(&self.anchor.para)).then(|| flow[self.anchor.para].check_id.clone());
        let before: Vec<(String, usize)> = flow[g.clone()].iter().enumerate().map(|(k, p)| (p.check_id.clone(), start + k)).collect();
        let flow = self.flow_mut(cell);
        let mut items: Vec<Para> = flow[g.clone()].to_vec();
        let sorted: Vec<Para> = if trigger && i >= start && i < g.end {
            // (NO2) the item just ticked goes to the top of the checked ones, right after the last
            // open one; nothing else moves
            let p = items.remove(i - start);
            let at = items.iter().rposition(|q| !q.checked).map_or(0, |k| k + 1);
            items.insert(at, p);
            items
        } else {
            // Move Checked to Bottom: open ones, then checked ones, each in their order
            let (open, done): (Vec<Para>, Vec<Para>) = items.into_iter().partition(|p| !p.checked);
            open.into_iter().chain(done).collect()
        };
        flow.splice(g.clone(), sorted);
        let at = |id: &Option<String>, flow: &Vec<Para>| id.as_ref().and_then(|id| flow[g.clone()].iter().position(|p| &p.check_id == id)).map(|j| start + j);
        let flow = self.flow(cell);
        let (hj, aj) = (at(&caret_id, flow), at(&anchor_id, flow));
        // what moved, from where: the window slides it from its old place
        let moved: Vec<(Option<Cell>, String, usize)> = before.into_iter().filter(|(id, old)| flow[*old].check_id != *id).map(|(id, old)| (cell, id, old)).collect();
        if let Some(j) = hj {
            self.head.para = j;
        }
        if let Some(j) = aj {
            self.anchor.para = j;
        }
        self.moved.extend(moved);
    }

    pub fn check_all(&mut self, on: bool) {
        self.push_undo(Kind::Other);
        let g = self.checklist_group(self.head.cell, self.head.para);
        let cell = self.head.cell;
        for i in g {
            self.flow_mut(cell)[i].checked = on;
        }
        self.changed = true;
    }

    /// Uncheck All / Check All with sorting moves nothing; Move Checked to Bottom does.
    pub fn move_checked_to_bottom(&mut self) {
        self.push_undo(Kind::Other);
        self.slide_timing = Some((0.25, 0.317));
        self.sort_checked_group(self.head.cell, self.head.para, false);
        self.changed = true;
    }

    pub fn delete_checked(&mut self) {
        self.push_undo(Kind::Other);
        let g = self.checklist_group(self.head.cell, self.head.para);
        let cell = self.head.cell;
        let start = g.start;
        let flow = self.flow_mut(cell);
        let kept: Vec<Para> = flow[g.clone()].iter().filter(|p| !p.checked).cloned().collect();
        let n = kept.len();
        flow.splice(g, kept);
        if flow.is_empty() {
            flow.push(Para::default());
        }
        self.head = self.clamp(Loc { cell, para: start + n.saturating_sub(1), off: usize::MAX });
        self.anchor = self.head;
        self.changed = true;
    }

    /// Format > Move Item > Up / Down: the selected paragraphs swap with the one above / below.
    pub fn move_item(&mut self, down: bool) {
        let r = self.sel_paras();
        let cell = self.head.cell;
        let n = self.flow(cell).len();
        if (!down && *r.start() == 0) || (down && *r.end() + 1 >= n) {
            return;
        }
        self.push_undo(Kind::Other);
        let flow = self.flow_mut(cell);
        if down {
            let p = flow.remove(*r.end() + 1);
            flow.insert(*r.start(), p);
        } else {
            let p = flow.remove(*r.start() - 1);
            flow.insert(*r.end(), p);
        }
        // the caret follows its paragraph: the moved ones shift by one, the one they passed goes
        // to the other side (a selection ending at a paragraph's start doesn't take it along)
        let (a, b) = (*r.start(), *r.end());
        for l in [&mut self.anchor, &mut self.head] {
            if l.cell != cell {
                continue;
            }
            l.para = match (down, l.para) {
                (true, p) if (a..=b).contains(&p) => p + 1,
                (true, p) if p == b + 1 => a,
                (false, p) if (a..=b).contains(&p) => p - 1,
                (false, p) if p + 1 == a => b,
                (_, p) => p,
            };
        }
        self.changed = true;
    }

    // ---- character styles

    /// Applies a character change to the selection, or to what is typed next.
    pub fn set_chars(&mut self, f: impl Fn(&mut Fmt)) {
        if !self.has_sel() {
            let mut p = self.typing_fmt();
            f(&mut p);
            self.pending = Some(p);
            return;
        }
        self.push_undo(Kind::Other);
        let (a, b) = self.range();
        let flow = self.flow_mut(a.cell);
        for i in a.para..=b.para {
            let s = if i == a.para { a.off } else { 0 };
            let e = if i == b.para { b.off } else { flow[i].len() };
            flow[i].set(s..e, &|x: &mut Fmt| f(x));
        }
        self.changed = true;
    }

    /// Every selected character (or the typing format) passes `f`.
    pub fn chars_all(&self, f: impl Fn(&Fmt) -> bool) -> bool {
        if !self.has_sel() {
            return f(&self.typing_fmt());
        }
        let (a, b) = self.range();
        let flow = self.flow(a.cell);
        (a.para..=b.para).all(|i| {
            let s = if i == a.para { a.off } else { 0 };
            let e = if i == b.para { b.off } else { flow[i].len() };
            s >= e || flow[i].all(s..e, &f)
        })
    }

    pub fn toggle_bold(&mut self) {
        let on = !self.chars_all(|f| f.bold);
        self.set_chars(|f| f.bold = on);
    }

    pub fn toggle_italic(&mut self) {
        let on = !self.chars_all(|f| f.italic);
        self.set_chars(|f| f.italic = on);
    }

    pub fn toggle_underline(&mut self) {
        let on = !self.chars_all(|f| f.underline);
        self.set_chars(|f| f.underline = on);
    }

    pub fn toggle_strike(&mut self) {
        let on = !self.chars_all(|f| f.strike);
        self.set_chars(|f| f.strike = on);
    }

    pub fn set_script(&mut self, v: i8) {
        self.set_chars(|f| f.script = v);
    }

    /// Format > Highlight / the Aa popover's pen: a colour, or off when it is on already.
    pub fn highlight(&mut self, c: Option<u8>) {
        let c = match c {
            Some(c) => Some(c),
            None if self.chars_all(|f| f.highlight.is_some()) => None,
            None => Some(self.last_highlight),
        };
        if let Some(c) = c {
            self.last_highlight = c;
        }
        let all_same = c.is_some() && self.chars_all(|f| f.highlight == c);
        self.set_chars(|f| f.highlight = if all_same { None } else { c });
    }

    pub fn set_link(&mut self, url: Option<String>) {
        if !self.has_sel() {
            // inside a link: the whole link
            let l = self.head;
            let p = self.para(l);
            let at = if l.off > 0 { l.off - 1 } else { l.off };
            let Some(cur) = p.fmt_of(at).and_then(|f| f.link.clone()) else {
                if let Some(u) = url {
                    // nothing selected: the address goes in as linked text
                    let fmt = Fmt { link: Some(u.clone()), ..self.typing_fmt() };
                    self.push_undo(Kind::Other);
                    self.insert_text(&u, &fmt);
                }
                return;
            };
            let (mut a, mut b) = (at, at);
            for (r, f) in p.spans() {
                if f.link.as_deref() == Some(cur.as_str()) && r.start <= b && r.end >= a {
                    a = a.min(r.start);
                    b = b.max(r.end);
                }
            }
            self.anchor = Loc { off: a, ..l };
            self.head = Loc { off: b, ..l };
        }
        self.set_chars(|f| f.link = url.clone());
    }

    // ---- blocks

    /// Puts a block attachment (table, divider, picture…) on its own line at the caret.
    pub fn insert_block(&mut self, att: Att) {
        self.push_undo(Kind::Other);
        self.delete_sel();
        let mut l = self.head;
        if l.cell.is_some() {
            l = Loc { cell: None, para: l.cell.unwrap().para, off: OBJ.len_utf8() };
            self.head = l;
        }
        self.leave_block_for_insert();
        let l = self.head;
        let id = att.id.clone();
        self.doc.atts.push(att);
        let cur = &self.doc.paras[l.para];
        let block = Para { text: String::new(), runs: Vec::new(), style: ParaStyle::Body, indent: cur.indent, ..Default::default() };
        let mut block = block;
        block.insert(0, "\u{FFFC}", &Fmt { att: Some(id), ..Default::default() });
        let at = if cur.is_empty() {
            self.doc.paras[l.para] = block;
            l.para
        } else {
            if l.off > 0 {
                self.head = l;
                self.split_para();
            }
            let at = self.head.para.max(if l.off > 0 { l.para + 1 } else { l.para });
            self.doc.paras.insert(at, block);
            at
        };
        // there is always a line after a block to type in
        if at + 1 >= self.doc.paras.len() || self.doc.block_of(&self.doc.paras[at + 1]).is_some() {
            self.doc.paras.insert(at + 1, Para::default());
        }
        self.head = Loc { cell: None, para: at + 1, off: 0 };
        self.anchor = self.head;
        self.changed = true;
    }

    fn leave_block_for_insert(&mut self) {
        let l = self.head;
        if self.doc.block_of(&self.doc.paras[l.para]).is_some() {
            self.doc.paras.insert(l.para + 1, Para::default());
            self.head = Loc { cell: None, para: l.para + 1, off: 0 };
        }
    }

    pub fn insert_divider(&mut self) {
        self.insert_block(Att::new(nt_core::model::UTI_DIVIDER, AttKind::Divider));
    }

    /// Format > Table: a 2 × 2 table, the caret in its first cell.
    pub fn insert_table(&mut self) {
        let att = Att::new(nt_core::model::UTI_TABLE, AttKind::Table(Table::new(2, 2)));
        let id = att.id.clone();
        self.insert_block(att);
        let para = self.doc.paras.iter().position(|p| p.sole_att() == Some(id.as_str())).unwrap_or(0);
        let l = Loc { cell: Some(Cell { para, row: 0, col: 0 }), para: 0, off: 0 };
        self.anchor = l;
        self.head = l;
    }

    // ---- collapsible sections

    /// The heading the caret's section belongs to.
    fn section_heading(&self) -> Option<usize> {
        let i = self.head.cell.map_or(self.head.para, |c| c.para);
        (0..=i).rev().find(|&j| self.doc.paras[j].is_heading())
    }

    pub fn set_collapsed(&mut self, i: usize, on: bool) {
        let Some(p) = self.doc.paras.get_mut(i).filter(|p| p.is_heading()) else { return };
        if p.uuid.is_empty() {
            p.uuid = new_uuid();
        }
        p.collapsed = on;
        if on {
            // the caret can't stay in what is hidden
            let hidden = super::layout::hidden_paras(&self.doc.paras);
            let cp = self.head.cell.map_or(self.head.para, |c| c.para);
            if hidden.get(cp).copied().unwrap_or(false) {
                let l = Loc { cell: None, para: i, off: self.doc.paras[i].len() };
                self.anchor = l;
                self.head = l;
            }
        }
        self.changed = true;
    }

    pub fn collapse_section(&mut self, on: bool) {
        if let Some(i) = self.section_heading() {
            self.set_collapsed(i, on);
        }
    }

    pub fn collapse_all(&mut self, on: bool) {
        for i in 0..self.doc.paras.len() {
            if self.doc.paras[i].is_heading() && i + 1 < self.doc.paras.len() {
                self.set_collapsed(i, on);
            }
        }
    }

    // ---- clipboard

    /// The selection as a fragment and as plain text.
    pub fn copy_frag(&self) -> Option<(Frag, String)> {
        if let Some(ts) = &self.table_sel {
            return self.table_sel_frag(ts);
        }
        if !self.has_sel() {
            return None;
        }
        let (a, b) = self.range();
        let flow = self.flow(a.cell);
        let mut paras = Vec::new();
        let mut plain = String::new();
        for i in a.para..=b.para {
            let s = if i == a.para { a.off } else { 0 };
            let e = if i == b.para { b.off } else { flow[i].len() };
            paras.push(flow[i].slice(s..e));
            if i > a.para {
                plain.push('\n');
            }
            plain.push_str(&self.doc.para_plain(&flow[i], s..e));
        }
        let ids: Vec<String> = paras.iter().flat_map(|p| p.runs.iter().filter_map(|r| r.fmt.att.clone())).collect();
        let atts = self.doc.atts.iter().filter(|a| ids.contains(&a.id)).cloned().collect();
        Some((Frag { paras, atts, open_end: b.off < flow[b.para].len() }, plain))
    }

    /// Pastes a fragment at the caret (attachments get new identities).
    pub fn paste_frag(&mut self, frag: &Frag) {
        if frag.paras.is_empty() {
            return;
        }
        // one picture / table / card: on its own line, as a copy
        if frag.paras.len() == 1 && self.head.cell.is_none() {
            if let Some(a) = frag.paras[0].sole_att().and_then(|id| frag.atts.iter().find(|a| a.id == id && a.is_block())) {
                let mut a = a.clone();
                a.id = new_uuid();
                a.model = None;
                self.insert_block(a);
                return;
            }
        }
        self.push_undo(Kind::Other);
        self.delete_sel();
        self.leave_block();
        let mut frag = frag.clone();
        for att in &mut frag.atts {
            let new = new_uuid();
            for p in &mut frag.paras {
                for r in &mut p.runs {
                    if r.fmt.att.as_deref() == Some(att.id.as_str()) {
                        r.fmt.att = Some(new.clone());
                    }
                }
            }
            att.id = new;
        }
        // no tables inside table cells
        if self.head.cell.is_some() {
            for p in &mut frag.paras {
                for r in &mut p.runs {
                    if r.fmt.att.as_ref().is_some_and(|id| frag.atts.iter().any(|a| &a.id == id && a.is_block())) {
                        r.fmt.att = None;
                    }
                }
            }
        }
        self.doc.atts.extend(frag.atts.iter().cloned());
        let l = self.head;
        let flow = self.flow_mut(l.cell);
        if frag.paras.len() == 1 {
            flow[l.para].insert_para(l.off, &frag.paras[0]);
            self.head = Loc { off: l.off + frag.paras[0].len(), ..l };
        } else {
            let tail = flow[l.para].split_off(l.off);
            let head_empty = flow[l.para].is_empty();
            if head_empty {
                // the first pasted paragraph keeps its own style
                flow[l.para] = frag.paras[0].clone();
            } else {
                flow[l.para].append(&frag.paras[0]);
            }
            let mut at = l.para;
            for p in &frag.paras[1..] {
                at += 1;
                flow.insert(at, p.clone());
            }
            let off = flow[at].len();
            flow[at].append(&tail);
            self.head = Loc { cell: l.cell, para: at, off };
        }
        self.anchor = self.head;
        self.changed = true;
    }

    /// Pastes plain text in the caret's format (Paste and Match Style, text from other apps).
    pub fn paste_plain(&mut self, text: &str) {
        self.push_undo(Kind::Other);
        let fmt = self.typing_fmt();
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        self.insert_text(&text, &fmt);
    }

    pub fn select_all(&mut self) {
        let cell = self.head.cell;
        let n = self.flow(cell).len();
        let last = self.flow(cell)[n - 1].len();
        self.anchor = Loc { cell, para: 0, off: 0 };
        self.head = Loc { cell, para: n - 1, off: last };
        if cell.is_some() && self.anchor == (Loc { cell, para: 0, off: 0 }) && self.head == self.anchor {
            self.anchor = Loc::default();
        }
    }
}

impl Editor {
    /// The note list's Tags menu: adds "#tag" at the end of the note, or removes its tags.
    pub fn toggle_tag(&mut self, tag: &str) {
        let name = tag.trim_start_matches('#').to_lowercase();
        let ids: Vec<String> = self.doc.atts.iter().filter(|a| matches!(&a.kind, AttKind::Tag(t) if t.trim_start_matches('#').to_lowercase() == name)).map(|a| a.id.clone()).collect();
        self.push_undo(Kind::Other);
        let used = self.doc.paras.iter().any(|p| p.runs.iter().any(|r| r.fmt.att.as_ref().is_some_and(|id| ids.contains(id))));
        if used {
            for p in &mut self.doc.paras {
                loop {
                    let r = p.spans().find(|(_, f)| f.att.as_ref().is_some_and(|id| ids.contains(id))).map(|(r, _)| r);
                    let Some(r) = r else { break };
                    p.remove(r);
                }
            }
        } else {
            let last = self.doc.paras.len() - 1;
            let n = self.doc.paras[last].len();
            let at = if n == 0 || self.doc.block_of(&self.doc.paras[last]).is_some() {
                self.doc.paras.push(Para::default());
                Loc { cell: None, para: last + 1, off: 0 }
            } else {
                self.doc.paras[last].insert(n, " ", &Fmt::default());
                Loc { cell: None, para: last, off: n + 1 }
            };
            self.anchor = at;
            self.head = at;
            self.insert_token(at.off..at.off, AttKind::Tag(format!("#{}", tag.trim_start_matches('#'))), nt_core::model::UTI_HASHTAG);
        }
        self.changed = true;
    }
}

/// Where the last word of `t` starts: after its last whitespace (which may be wider than a
/// byte: a no-break space).
pub fn after_last_space(t: &str) -> usize {
    t.char_indices().rev().find(|(_, c)| c.is_whitespace()).map_or(0, |(i, c)| i + c.len_utf8())
}

/// Text replacements Notes does by default (Edit > Substitutions > Text Replacement): the
/// system's ("omw") and "..." to an ellipsis.
fn replacement(before: &str) -> Option<(usize, &'static str)> {
    if let Some(s) = before.strip_suffix("...") {
        return Some((s.len(), "\u{2026}"));
    }
    let start = after_last_space(before);
    match &before[start..] {
        "omw" => Some((start, "On my way!")),
        _ => None,
    }
}

/// Where an address typed in a note links (Smart Links), if it is one: a URL with its scheme,
/// "www.…", a bare domain with a known top-level domain, an email address.
pub fn link_target(w: &str) -> Option<String> {
    if w.is_empty() || w.chars().any(|c| c.is_whitespace() || c == OBJ) {
        return None;
    }
    let lower = w.to_ascii_lowercase();
    for scheme in ["http://", "https://", "ftp://", "mailto:", "tel:", "file://"] {
        if lower.starts_with(scheme) {
            return (w.len() > scheme.len() + 1).then(|| w.to_string());
        }
    }
    if let Some((user, host)) = w.split_once('@') {
        let ok = !user.is_empty() && user.chars().all(|c| c.is_alphanumeric() || "._%+-".contains(c)) && is_domain(host);
        return ok.then(|| format!("mailto:{w}"));
    }
    let host = w.split(['/', '?', '#']).next().unwrap_or(w);
    let host = host.split(':').next().unwrap_or(host);
    if lower.starts_with("www.") && host.len() > 4 && host[4..].contains('.') && is_domain(host) {
        return Some(format!("http://{w}"));
    }
    is_domain(host).then(|| format!("http://{w}"))
}

/// "example.com", "bbc.co.uk": labels of letters, digits and hyphens, the last a known TLD.
fn is_domain(h: &str) -> bool {
    const TLDS: &[&str] = &[
        "com", "org", "net", "edu", "gov", "mil", "int", "io", "co", "ai", "app", "dev", "me", "info", "biz", "tv", "us", "uk", "ca", "au", "nz", "de", "fr",
        "es", "it", "nl", "be", "ch", "at", "se", "no", "dk", "fi", "ie", "pl", "pt", "jp", "cn", "kr", "in", "br", "mx", "ru", "eu", "xyz", "online", "site", "tech", "store", "blog",
    ];
    let labels: Vec<&str> = h.split('.').collect();
    labels.len() >= 2
        && labels.iter().all(|l| !l.is_empty() && !l.starts_with('-') && !l.ends_with('-') && l.chars().all(|c| c.is_alphanumeric() || c == '-'))
        && TLDS.contains(&labels[labels.len() - 1].to_ascii_lowercase().as_str())
        && !labels[0].chars().all(|c| c.is_ascii_digit())
}

/// A link Smart Links would make (its target an address, not something chosen in Add Link).
fn was_address(link: &str) -> bool {
    let l = link.to_ascii_lowercase();
    l.starts_with("http://") || l.starts_with("https://") || l.starts_with("mailto:")
}
