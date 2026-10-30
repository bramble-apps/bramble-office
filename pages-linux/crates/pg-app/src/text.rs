//! Typing and moving the caret in the body, text boxes, headers, footers and footnotes:
//! insert / delete (Track Changes aware), Return (new paragraph with the following style;
//! an empty list item ends the list), Tab (list levels), breaks, smart quotes and dashes,
//! text replacements, auto-capitalisation and list detection (Settings > Auto-Correction),
//! caret movement by character / word / line / paragraph / page, clipboard with formatting.

use std::cell::Cell;

use egui::{Event, Key, Modifiers};
use pg_core::kn_format::ObjId;
use pg_core::kn_model::{Drawable, DrawableKind, TextBox};

use crate::app::{App, Cursor, Owner};

thread_local! {
    /// Tab pressed (with Shift?) this frame, taken out of egui's input so it reaches the text.
    static TAB: Cell<Option<bool>> = const { Cell::new(None) };
}

/// A character attribute picked with nothing selected: applied to what is typed next.
#[derive(Clone, Debug, PartialEq)]
pub enum CharChange {
    Bold(bool),
    Italic(bool),
    Underline(bool),
    Strike(bool),
    Size(f32),
    Font(String),
    Color(pg_core::kn_model::Color),
    Script(i8),
}

impl CharChange {
    pub fn apply(&self, cp: &mut pg_core::kn_format::Msg) {
        match self {
            CharChange::Bold(b) => cp.set_bool("bold", *b),
            CharChange::Italic(b) => cp.set_bool("italic", *b),
            CharChange::Underline(b) => cp.set_enum("underline", if *b { "kSingleUnderline" } else { "kNoUnderline" }),
            CharChange::Strike(b) => cp.set_enum("strikethru", if *b { "kSingleStrikethru" } else { "kNoStrikethru" }),
            CharChange::Size(s) => cp.set_f32("font_size", s.clamp(1.0, 1638.0)),
            CharChange::Font(f) => cp.set_str("font_name", f),
            CharChange::Color(c) => {
                cp.remove("font_color");
                cp.push_msg("font_color", pg_core::kn_model::format::color_msg(*c));
            }
            CharChange::Script(v) => cp.set_enum("superscript", match v {
                1 => "kSuperscript",
                -1 => "kSubscript",
                _ => "kNoScript",
            }),
        }
    }
}

/// Copied text with its character runs (pasted with its formatting inside Letters).
#[derive(Clone, Debug)]
pub struct TextClip {
    pub text: String,
    /// The stored text copied (breaks and all).
    pub stored: String,
    /// (byte start in `stored`, character style) runs.
    pub runs: Vec<(usize, Option<ObjId>)>,
    /// (byte start, paragraph style) of each paragraph.
    pub paras: Vec<(usize, Option<ObjId>)>,
    /// (byte start, list style) of each paragraph.
    pub lists: Vec<(usize, Option<ObjId>)>,
    pub doc: String,
}

/// The typing step in progress ends (the next edit is a new undo step).
pub fn finish_typing(app: &mut App) {
    if let Some(w) = &mut app.wb {
        w.doc.end(&mut app.hist);
    }
}

/// Keep Tab for the text (egui would move the keyboard focus with it).
pub fn take_keys(app: &mut App, raw: &mut egui::RawInput) {
    if (app.cursor.is_none() && app.sel.is_none()) || app.widget_focus {
        return;
    }
    raw.events.retain(|e| match e {
        Event::Key { key: Key::Tab, pressed, modifiers, .. } if !modifiers.command => {
            if *pressed {
                TAB.with(|t| t.set(Some(modifiers.shift)));
            }
            false
        }
        _ => true,
    });
}

/// Format > Font > Capitalization `name` ("None", "All Caps", "Small Caps", "Title Case",
/// "Start Case") as character properties: title and start case are both kTitled, start case
/// without the linguistic rules (small words capitalised too).
pub fn set_caps(cp: &mut pg_core::kn_format::Msg, name: &str) {
    let v = match name {
        "All Caps" => "kAllCaps",
        "Small Caps" => "kSmallCaps",
        "Title Case" | "Start Case" => "kTitled",
        _ => "kNoCaps",
    };
    cp.set_enum("capitalization", v);
    if v == "kTitled" {
        cp.set_bool("capitalization_uses_linguistics", name == "Title Case");
    } else {
        cp.remove("capitalization_uses_linguistics");
    }
}

/// A text box or shape's text.
pub fn drawable_text(d: &Drawable) -> Option<&TextBox> {
    match &d.kind {
        DrawableKind::Shape(s) => s.text.as_ref(),
        _ => None,
    }
}

/// Stored byte -> shown byte in a text box (fields shown as their value).
pub fn shown_of(tb: &TextBox, stored: usize) -> usize {
    let mut delta: i64 = 0;
    for &(at, len, raw) in &tb.fields {
        if raw < stored {
            delta += len as i64 - '\u{FFFC}'.len_utf8() as i64;
        }
        let _ = at;
    }
    (stored as i64 + delta).max(0) as usize
}

fn prev_boundary(t: &str, i: usize) -> usize {
    let i = i.min(t.len());
    t[..i].char_indices().next_back().map(|(b, _)| b).unwrap_or(0)
}

fn next_boundary(t: &str, i: usize) -> usize {
    let i = i.min(t.len());
    t[i..].chars().next().map(|c| i + c.len_utf8()).unwrap_or(t.len())
}

fn prev_word(t: &str, i: usize) -> usize {
    let mut j = i.min(t.len());
    while j > 0 && !t[..j].chars().next_back().unwrap().is_alphanumeric() {
        j = prev_boundary(t, j);
    }
    while j > 0 && t[..j].chars().next_back().unwrap().is_alphanumeric() {
        j = prev_boundary(t, j);
    }
    j
}

fn next_word(t: &str, i: usize) -> usize {
    let mut j = i.min(t.len());
    while j < t.len() && !t[j..].chars().next().unwrap().is_alphanumeric() {
        j = next_boundary(t, j);
    }
    while j < t.len() && t[j..].chars().next().unwrap().is_alphanumeric() {
        j = next_boundary(t, j);
    }
    j
}

fn storage_text(app: &App, st: ObjId) -> String {
    app.doc().map(|d| d.storage_text(st)).unwrap_or_default()
}

/// Plain text of stored bytes a..b (breaks as new lines, attachments left out).
pub fn plain(stored: &str) -> String {
    stored
        .chars()
        .filter_map(|c| match c {
            '\u{FFFC}' | '\u{E}' => None,
            '\u{5}' | '\u{c}' | '\u{4}' | '\r' | '\u{2028}' | '\u{2029}' => Some('\n'),
            c => Some(c),
        })
        .collect()
}

/// The author name for comments and tracked changes.
pub fn author(app: &App) -> String {
    // scripted runs make files that get shared: never the user's name in them
    if std::env::var_os("CS_SNAP").is_some() || std::env::var_os("CS_SCRIPT").is_some() {
        return "Test Author".to_string();
    }
    let a = app.settings.author.trim();
    if a.is_empty() { "Author".to_string() } else { a.to_string() }
}

/// Replace the cursor's selection with `s` (typing): the core edit, Track Changes aware.
/// `merge`: continue the typing undo step.
pub fn replace_selection(app: &mut App, s: &str, label: &str, merge: bool) {
    let Some(c) = app.cursor else { return };
    let tracking = c.owner == Owner::Body && app.wb.as_ref().is_some_and(|w| w.tracking());
    let who = author(app);
    let props = std::mem::take(&mut app.typing_props);
    // a colour or size being dragged: one undo step
    let merge = merge || app.pointer_down;
    let Some(w) = app.wb.as_mut() else { return };
    if merge {
        w.doc.begin_merge(&mut app.hist, label);
    } else {
        w.doc.begin(&mut app.hist, label);
    }
    let (a, b) = c.range();
    let st = c.storage;
    let mut at = a;
    if tracking {
        if b > a {
            let before = w.doc.storage_text(st).len();
            w.tracked_delete(&mut app.hist, st, a, b, &who);
            // deleted text stays (marked): typing goes after it
            at = if w.doc.storage_text(st).len() == before { b } else { a };
        }
        if !s.is_empty() {
            w.tracked_insert(&mut app.hist, st, at, s, &who);
        }
    } else {
        pg_core::edit::replace(&mut w.doc, &mut app.hist, st, a, b, s);
    }
    let end = at + s.len();
    if !props.is_empty() && !s.is_empty() {
        pg_core::edit::set_char_props(&mut w.doc, &mut app.hist, st, at, end, &|cp| {
            for p in &props {
                p.apply(cp);
            }
        });
    }
    w.doc.end(&mut app.hist);
    app.cursor = Some(Cursor { anchor: end, caret: end, goal_x: None, ..c });
    app.changed();
    app.canvas.follow_caret = true;
}

/// Typed text at the caret, with Settings > Auto-Correction (a character at a time, as typed).
pub fn type_text(app: &mut App, t: &str) {
    if t.chars().count() > 1 {
        for ch in t.chars() {
            type_text(app, ch.encode_utf8(&mut [0u8; 4]));
        }
        return;
    }
    let Some(c) = app.cursor else { return };
    if app.canvas.obj.is_some() && !matches!(c.owner, Owner::Drawable(_)) {
        // an object is selected: typing replaces nothing (Pages beeps)
        return;
    }
    let text = storage_text(app, c.storage);
    let (a, _) = c.range();
    let before = text[..a.min(text.len())].chars().next_back();
    let (take, mut ins) = kn_ui::typing::smart_text(before, t, c.is_empty(), app.settings.smart_quotes, app.settings.smart_dashes);
    if take > 0 {
        // "--" -> em dash: the first hyphen goes
        let from = prev_boundary(&text, a);
        app.cursor = Some(Cursor { anchor: from, caret: c.caret.max(a), ..c });
    }
    // capitalise the first letter of a sentence
    if app.settings.capitalize && ins.chars().count() == 1 && ins.chars().all(|ch| ch.is_lowercase()) {
        let head = &text[..a.min(text.len())];
        let trimmed = head.trim_end_matches(|ch: char| ch == ' ');
        let starts = trimmed.is_empty() || trimmed.ends_with(['.', '!', '?']) || trimmed.ends_with(|ch: char| pg_core::body::is_para_end(ch));
        if starts && (head.len() > trimmed.len() || trimmed.is_empty() || trimmed.ends_with(|ch: char| pg_core::body::is_para_end(ch))) {
            ins = ins.to_uppercase();
        }
    }
    replace_selection(app, &ins, "Typing", true);
    // a word ended: text replacements and spelling fixes
    if t.chars().all(|ch| ch == ' ' || ".,;:!?)".contains(ch)) {
        let Some(c) = app.cursor else { return };
        let text = storage_text(app, c.storage);
        let pos = prev_boundary(&text, c.caret);
        // (word_fix gives the byte where the word starts and what it becomes)
        if let Some((start, with)) = kn_ui::typing::word_fix(&text[..pos], app.settings.text_replacement, app.settings.autocorrect && app.settings.spell_check) {
            let tail = text[pos..c.caret].to_string();
            app.cursor = Some(Cursor { anchor: start, caret: c.caret, ..c });
            replace_selection(app, &format!("{with}{tail}"), "Typing", true);
        }
        // list detection: "* ", "- ", "1. " typed at a paragraph start
        if app.settings.detect_lists && t == " " && c.owner == Owner::Body {
            detect_list(app);
        }
    }
}

fn detect_list(app: &mut App) {
    let Some(c) = app.cursor else { return };
    let text = storage_text(app, c.storage);
    let start = text[..c.caret].rfind(|ch: char| pg_core::body::is_para_end(ch)).map(|i| i + 1).unwrap_or(0);
    let head = &text[start..c.caret];
    let kind = match head {
        "* " | "- " | "• " => "Bullet",
        "1. " | "1) " => "Numbered",
        "a. " | "a) " | "A. " => "Lettered",
        _ => return,
    };
    let Some(w) = app.wb.as_ref() else { return };
    let Some(list) = pg_core::edit::list_styles(&w.doc).into_iter().find(|(_, n)| n == kind).map(|x| x.0) else { return };
    let st = c.storage;
    let n = head.len();
    app.edit("List", |d, h| {
        pg_core::edit::replace(d, h, st, start, start + n, "");
        pg_core::edit::set_list_style(d, h, st, start, start, Some(list));
    });
    app.cursor = Some(Cursor { anchor: start, caret: start, ..c });
}

/// Return: a new paragraph (the following style; Return in an empty list item ends the list).
pub fn new_paragraph(app: &mut App) {
    let Some(c) = app.cursor else { return };
    let st = c.storage;
    let text = storage_text(app, st);
    let (a, _) = c.range();
    let pstart = text[..a].rfind(|ch: char| pg_core::body::is_para_end(ch)).map(|i| i + 1).unwrap_or(0);
    let pend = text[a..].find(|ch: char| pg_core::body::is_para_end(ch)).map(|i| a + i).unwrap_or(text.len());
    let empty_para = pstart == pend && c.is_empty();
    // the list of the paragraph
    let in_list = app.layout.as_ref().and_then(|l| l.body.as_ref()).filter(|_| c.owner == Owner::Body).is_some_and(|b| {
        let pi = b.para_at_stored(a);
        b.tb.paragraphs.get(pi).is_some_and(|p| p.label.is_some())
    });
    if empty_para && in_list {
        app.edit("List", |d, h| {
            pg_core::edit::set_list_style(d, h, st, a, a, None);
        });
        return;
    }
    // following paragraph style (Heading -> Body), at the end of a paragraph
    let following = app.doc().and_then(|d| {
        let ps = pg_core::edit::para_style_at(d, st, a)?;
        let ch = pg_core::kn_model::style::chain(&d.file, Some(ps));
        pg_core::kn_model::style::lookup(&ch, |m| m.msg("para_properties").and_then(|p| p.ref_id("following_style"))).filter(|f| *f != ps)
    });
    replace_selection(app, "\n", "Typing", true);
    if let (Some(f), true) = (following, a == pend) {
        let at = a + 1;
        if let Some(w) = app.wb.as_mut() {
            w.doc.begin_merge(&mut app.hist, "Typing");
            pg_core::edit::set_para_style(&mut w.doc, &mut app.hist, st, at, at, f);
            w.doc.end(&mut app.hist);
        }
        app.changed();
    }
}

/// Backspace (forward: Delete); `word`: Ctrl / Alt deletes a word.
pub fn delete(app: &mut App, forward: bool, word: bool) {
    let Some(c) = app.cursor else { return };
    if !c.is_empty() {
        replace_selection(app, "", "Delete", false);
        return;
    }
    let text = storage_text(app, c.storage);
    // Backspace at the start of an indented list item: one level out
    if !forward && c.owner == Owner::Body {
        let pstart = text[..c.caret].rfind(|ch: char| pg_core::body::is_para_end(ch)).map(|i| i + 1).unwrap_or(0);
        if pstart == c.caret {
            let level = app.layout.as_ref().and_then(|l| l.body.as_ref()).map(|b| {
                let pi = b.para_at_stored(c.caret);
                b.tb.paragraphs.get(pi).map(|p| (p.level, p.label.is_some())).unwrap_or((0, false))
            });
            if let Some((lvl, true)) = level {
                let st = c.storage;
                if lvl > 0 {
                    app.edit("Indent", |d, h| pg_core::edit::change_level(d, h, st, c.caret, c.caret, -1));
                } else {
                    app.edit("List", |d, h| pg_core::edit::set_list_style(d, h, st, c.caret, c.caret, None));
                }
                return;
            }
        }
    }
    let (a, b) = if forward {
        (c.caret, if word { next_word(&text, c.caret) } else { next_boundary(&text, c.caret) })
    } else {
        (if word { prev_word(&text, c.caret) } else { prev_boundary(&text, c.caret) }, c.caret)
    };
    if a == b {
        return;
    }
    app.cursor = Some(Cursor { anchor: a, caret: b, ..c });
    replace_selection(app, "", "Delete", true);
}

/// Move the caret; `extend`: Shift keeps the anchor.
fn move_to(app: &mut App, to: usize, extend: bool, keep_goal: bool) {
    let Some(c) = app.cursor else { return };
    let anchor = if extend { c.anchor } else { to };
    app.cursor = Some(Cursor { anchor, caret: to, goal_x: if keep_goal { c.goal_x } else { None }, ..c });
    app.canvas.follow_caret = true;
    app.typing_props.clear();
}

/// The stored offset at page point (x, y) for the cursor's owner on page `page`.
fn pos_at(app: &mut App, c: &Cursor, page: usize, x: f32, y: f32) -> Option<usize> {
    let l = app.layout.as_ref()?;
    match c.owner {
        Owner::Body => {
            let body = l.body.as_ref()?;
            let s = l.hit(page, x, y)?;
            Some(body.to_stored(s))
        }
        Owner::HeaderFooter { page } | Owner::Footnote { page } => {
            let p = l.pages.get(page)?;
            let (pc, body, dx, dy) = p
                .headers
                .iter()
                .chain(&p.footers)
                .find(|h| h.storage == c.storage)
                .map(|h| (h.piece.clone(), h.body.clone(), h.dx, h.dy))
                .or_else(|| p.footnotes.iter().find(|f| f.storage == c.storage).map(|f| (f.piece.clone(), f.body.clone(), f.dx, f.dy)))?;
            Some(body.to_stored(pg_core::flow::piece_hit(&pc, x - dx, y - dy)))
        }
        Owner::Drawable(id) => {
            let d = app.drawables.iter().find(|d| d.id == id)?.clone();
            let tb = drawable_text(&d)?.clone();
            let (tl, g) = app.canvas.tiles.renderer.text_layout(&d)?;
            Some(tb.stored_offset(kn_render::text::hit_index(&tl, x - g.x, y - g.y)))
        }
    }
}

/// Up / Down by a line (Page Up / Down by a view's height).
fn move_vertical(app: &mut App, dir: f32, by_view: bool, extend: bool) {
    let Some(c) = app.cursor else { return };
    let Some(l) = app.layout.take() else { return };
    let rect = crate::canvas::caret_rect(app, &l, &c);
    app.layout = Some(l);
    let Some((page, x, top, h)) = rect else { return };
    let goal = c.goal_x.unwrap_or(x);
    let step = if by_view { app.canvas_view_height() * 0.9 } else { h * 0.5 + 2.0 };
    let mut y = if dir < 0.0 { top - step } else { top + h + step - if by_view { h } else { 0.0 } };
    let mut pg = page;
    let ph = app.layout.as_ref().map(|l| l.setup.height).unwrap_or(792.0);
    let n = app.page_count();
    while y < 0.0 && pg > 0 && c.owner == Owner::Body {
        pg -= 1;
        y += ph;
    }
    while y > ph && pg + 1 < n && c.owner == Owner::Body {
        pg += 1;
        y -= ph;
    }
    let text_len = storage_text(app, c.storage).len();
    let to = if y < 0.0 && (pg == 0 || c.owner != Owner::Body) {
        Some(if c.owner == Owner::Body { 0 } else { pos_at(app, &c, pg, goal, top).unwrap_or(0) })
    } else if y > ph && pg + 1 >= n {
        Some(text_len)
    } else {
        pos_at(app, &c, pg, goal, y)
    };
    if let Some(mut to) = to {
        // a line with nowhere further to go: the start / end of the text
        if to == c.caret {
            to = if dir < 0.0 { if by_view { 0 } else { c.caret } } else if by_view { text_len } else { c.caret };
            if to == c.caret {
                let lines_end = if dir < 0.0 { 0 } else { text_len };
                if (dir < 0.0 && top < 24.0) || dir > 0.0 {
                    to = lines_end;
                }
            }
        }
        move_to(app, to, extend, true);
        if let Some(cc) = app.cursor.as_mut() {
            cc.goal_x = Some(goal);
        }
    }
}

/// Home / End: the start / end of the caret's line.
fn line_edge(app: &mut App, end: bool, extend: bool) {
    let Some(c) = app.cursor else { return };
    let to = match (c.owner, app.layout.as_ref()) {
        (Owner::Body, Some(l)) => l.body.as_ref().and_then(|b| {
            let s = b.to_shown(c.caret);
            let (_, line) = l.line_at(s)?;
            let (a, e) = line.range();
            Some(b.to_stored(if end { e } else { a }))
        }),
        _ => {
            let t = storage_text(app, c.storage);
            Some(if end {
                t[c.caret..].find(|ch: char| pg_core::body::is_para_end(ch) || ch == '\u{2028}').map(|i| c.caret + i).unwrap_or(t.len())
            } else {
                t[..c.caret].rfind(|ch: char| pg_core::body::is_para_end(ch) || ch == '\u{2028}').map(|i| i + 1).unwrap_or(0)
            })
        }
    };
    if let Some(to) = to {
        move_to(app, to, extend, false);
    }
}

impl App {
    pub fn canvas_view_height(&self) -> f32 {
        self.canvas.size.y / self.canvas.zoom
    }
}

/// The selected stored text.
pub fn selected_stored(app: &App) -> Option<String> {
    let c = app.cursor?;
    if c.is_empty() {
        return None;
    }
    let t = storage_text(app, c.storage);
    let (a, b) = c.range();
    t.get(a..b).map(str::to_string)
}

/// Copy (and Cut) the selected text: plain text to the system clipboard, formatting kept for
/// pastes inside Letters.
pub fn copy(app: &mut App, ctx: &egui::Context, cut: bool) -> bool {
    let Some(c) = app.cursor.filter(|c| !c.is_empty()) else { return false };
    let Some(stored) = selected_stored(app) else { return false };
    let (a, b) = c.range();
    let doc = app.doc().unwrap();
    let full = doc.storage_text(c.storage);
    let st = doc.file.obj(c.storage);
    let u = |byte: usize| full[..byte].encode_utf16().count();
    let to_byte = |u16i: usize| {
        let mut n = 0;
        for (bi, ch) in full.char_indices() {
            if n >= u16i {
                return bi;
            }
            n += ch.len_utf16();
        }
        full.len()
    };
    let table = |name: &str| -> Vec<(usize, Option<ObjId>)> {
        let mut v: Vec<(usize, Option<ObjId>)> = st.and_then(|s| s.msg(name)).map(|t| t.msgs("entries").map(|e| (e.u64("character_index").unwrap_or(0) as usize, e.ref_id("object"))).collect()).unwrap_or_default();
        v.sort_by_key(|e| e.0);
        v
    };
    let (ua, ub) = (u(a), u(b));
    let pick = |t: Vec<(usize, Option<ObjId>)>, carry: bool| {
        let mut out = Vec::new();
        let mut last = None;
        for (ci, o) in &t {
            if *ci <= ua {
                if !carry || o.is_some() {
                    last = *o;
                }
            }
        }
        out.push((0, last));
        for (ci, o) in t {
            if ci > ua && ci < ub {
                out.push((to_byte(ci) - a, o));
            }
        }
        out
    };
    let runs = pick(table("table_char_style"), false);
    let paras = pick(table("table_para_style"), true);
    let lists = pick(table("table_list_style"), true);
    let text = plain(&stored);
    ctx.copy_text(text.clone());
    app.text_clip = Some(TextClip { text, stored, runs, paras, lists, doc: pg_core::kn_model::insert::doc_identity(doc) });
    app.obj_clip_active = false;
    if cut {
        replace_selection(app, "", "Cut", false);
    }
    true
}

/// Paste text at the caret: Letters' own copy keeps its formatting (unless `match_style`).
pub fn paste(app: &mut App, text: &str, match_style: bool) {
    let Some(c) = app.cursor else { return };
    let ours = app.text_clip.clone().filter(|cl| cl.text == text.replace("\r\n", "\n") && !match_style && app.doc().is_some_and(|d| pg_core::kn_model::insert::doc_identity(d) == cl.doc));
    match ours {
        Some(clip) => {
            let st = c.storage;
            let at = c.range().0;
            replace_selection(app, &clip.stored, "Paste", false);
            let Some(w) = app.wb.as_mut() else { return };
            w.doc.begin(&mut app.hist, "Paste");
            for (k, (off, style)) in clip.runs.iter().enumerate() {
                let end = clip.runs.get(k + 1).map(|r| r.0).unwrap_or(clip.stored.len());
                if end > *off {
                    w.doc.set_character_style(&mut app.hist, st, at + off, at + end, *style);
                }
            }
            let starts_at = |w: &pg_core::Pages, k: usize, off: usize| k > 0 || off == 0 && (at == 0 || w.doc.storage_text(st)[..at].ends_with(|ch: char| pg_core::body::is_para_end(ch)));
            for (k, (off, l)) in clip.lists.iter().enumerate() {
                if starts_at(w, k, *off) {
                    pg_core::edit::set_list_style(&mut w.doc, &mut app.hist, st, at + off, at + off, *l);
                }
            }
            for (k, (off, style)) in clip.paras.iter().enumerate() {
                if let Some(s) = style {
                    // whole pasted paragraphs take their style
                    let starts_para = k > 0 || *off == 0 && (at == 0 || w.doc.storage_text(st)[..at].ends_with(|ch: char| pg_core::body::is_para_end(ch)));
                    if starts_para {
                        pg_core::edit::set_para_style(&mut w.doc, &mut app.hist, st, at + off, at + off, *s);
                    }
                }
            }
            w.doc.end(&mut app.hist);
            app.changed();
        }
        None => {
            let t = text.replace("\r\n", "\n").replace('\r', "\n");
            // a single-line field (header) keeps paragraphs; tables of text paste as is
            replace_selection(app, &t, "Paste", false);
        }
    }
}

/// Start editing a text box / shape's text (double-click, Return on a selected box).
pub fn edit_drawable_text(app: &mut App, id: ObjId, at: Option<egui::Pos2>) {
    let Some(d) = app.drawables.iter().find(|d| d.id == id).cloned() else { return };
    let Some(tb) = drawable_text(&d) else { return };
    let st = tb.storage;
    let len = storage_text(app, st).len();
    let pos = at
        .and_then(|p| {
            let (tl, g) = app.canvas.tiles.renderer.text_layout(&d)?;
            Some(tb.stored_offset(kn_render::text::hit_index(&tl, p.x - g.x, p.y - g.y)))
        })
        .unwrap_or(len);
    finish_typing(app);
    app.canvas.obj = Some(id);
    app.canvas.objs = vec![id];
    app.canvas.editing = Some(id);
    app.cursor = Some(Cursor { storage: st, owner: Owner::Drawable(id), anchor: pos, caret: pos, goal_x: None });
}

/// Keyboard and text input for the caret (called by the canvas each frame).
pub fn handle_input(app: &mut App, ctx: &egui::Context, _focused: bool) {
    if crate::dialogs::modal_open(app) || ctx.egui_wants_keyboard_input() {
        TAB.with(|t| t.set(None));
        return;
    }
    let tab = TAB.with(|t| t.take());
    // table cells take the keys while selected
    if app.sel.is_some() && app.cursor.is_none() {
        crate::tables::handle_keys(app, ctx, tab);
        return;
    }
    let events = ctx.input(|i| i.events.clone());
    let objects_selected = app.canvas.obj.is_some() && app.canvas.editing.is_none();
    if objects_selected {
        crate::insert::object_keys(app, ctx, &events);
        return;
    }
    if app.cursor.is_none() {
        return;
    }
    if let Some(shift) = tab {
        tab_key(app, shift);
    }
    for e in events {
        match e {
            Event::Text(t) => {
                if !t.chars().all(|c| c.is_control()) {
                    type_text(app, &t);
                    app.canvas.blink = ctx.input(|i| i.time);
                }
            }
            Event::Ime(egui::ImeEvent::Commit(t)) => type_text(app, &t),
            Event::Paste(t) => paste(app, &t, false),
            Event::Copy => {
                copy(app, ctx, false);
            }
            Event::Cut => {
                copy(app, ctx, true);
            }
            Event::Key { key, pressed: true, modifiers, .. } => {
                key_press(app, ctx, key, modifiers);
                app.canvas.blink = ctx.input(|i| i.time);
            }
            _ => {}
        }
    }
}

fn tab_key(app: &mut App, shift: bool) {
    let Some(c) = app.cursor else { return };
    // in a list paragraph (caret at its start or text selected): change the level
    let list_para = c.owner == Owner::Body
        && app.layout.as_ref().and_then(|l| l.body.as_ref()).is_some_and(|b| {
            let pi = b.para_at_stored(c.caret);
            b.tb.paragraphs.get(pi).is_some_and(|p| p.label.is_some())
        });
    let text = storage_text(app, c.storage);
    let at_start = text[..c.range().0].ends_with(|ch: char| pg_core::body::is_para_end(ch)) || c.range().0 == 0;
    if list_para && (at_start || !c.is_empty()) {
        let (a, b) = c.range();
        let st = c.storage;
        app.edit("Indent", |d, h| pg_core::edit::change_level(d, h, st, a, b, if shift { -1 } else { 1 }));
        return;
    }
    if !shift {
        replace_selection(app, "\t", "Typing", true);
    }
}

fn key_press(app: &mut App, ctx: &egui::Context, key: Key, m: Modifiers) {
    let Some(c) = app.cursor else { return };
    // Ctrl with other keys is a menu shortcut (Page Break, Select All…): the menus run those
    if m.command && !matches!(key, Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown | Key::Home | Key::End | Key::Backspace | Key::Delete) {
        return;
    }
    let text = storage_text(app, c.storage);
    let word = m.command || m.alt;
    match key {
        Key::Backspace => delete(app, false, word),
        Key::Delete => delete(app, true, word),
        Key::Enter if m.shift => replace_selection(app, "\u{2028}", "Typing", true),
        Key::Enter => {
            if matches!(c.owner, Owner::HeaderFooter { .. }) {
                replace_selection(app, "\u{2028}", "Typing", true);
            } else {
                new_paragraph(app);
            }
        }
        Key::ArrowLeft => {
            let to = if !c.is_empty() && !m.shift { c.range().0 } else if word { prev_word(&text, c.caret) } else { prev_boundary(&text, c.caret) };
            move_to(app, to, m.shift, false);
        }
        Key::ArrowRight => {
            let to = if !c.is_empty() && !m.shift { c.range().1 } else if word { next_word(&text, c.caret) } else { next_boundary(&text, c.caret) };
            move_to(app, to, m.shift, false);
        }
        Key::ArrowUp if m.command => move_to(app, 0, m.shift, false),
        Key::ArrowDown if m.command => move_to(app, text.len(), m.shift, false),
        Key::ArrowUp if m.alt => {
            let to = text[..prev_boundary(&text, c.caret)].rfind(|ch: char| pg_core::body::is_para_end(ch)).map(|i| i + 1).unwrap_or(0);
            move_to(app, to, m.shift, false);
        }
        Key::ArrowDown if m.alt => {
            let to = text[next_boundary(&text, c.caret).min(text.len())..].find(|ch: char| pg_core::body::is_para_end(ch)).map(|i| next_boundary(&text, c.caret) + i).unwrap_or(text.len());
            move_to(app, to, m.shift, false);
        }
        Key::ArrowUp => move_vertical(app, -1.0, false, m.shift),
        Key::ArrowDown => move_vertical(app, 1.0, false, m.shift),
        Key::PageUp => move_vertical(app, -1.0, true, m.shift),
        Key::PageDown => move_vertical(app, 1.0, true, m.shift),
        Key::Home if m.command => move_to(app, 0, m.shift, false),
        Key::End if m.command => move_to(app, text.len(), m.shift, false),
        Key::Home => line_edge(app, false, m.shift),
        Key::End => line_edge(app, true, m.shift),
        Key::Escape => {
            finish_typing(app);
            match c.owner {
                // out of a text box's text: the box is selected (Pages)
                Owner::Drawable(id) => {
                    app.cursor = None;
                    app.canvas.editing = None;
                    app.canvas.obj = Some(id);
                    app.canvas.objs = vec![id];
                }
                Owner::HeaderFooter { .. } | Owner::Footnote { .. } => {
                    // back to the body: after a footnote's mark, else where the body caret was
                    let at = app.layout.as_ref().and_then(|l| l.body.as_ref()).and_then(|b| {
                        b.spans.iter().find_map(|s| match s.kind {
                            pg_core::body::SpanKind::Footnote { storage, .. } if storage == c.storage => Some(b.to_stored(s.end)),
                            _ => None,
                        })
                    });
                    let body = app.wb.as_ref().and_then(|w| w.body_storage()).filter(|_| app.wb.as_ref().is_some_and(|w| w.is_word_processing()));
                    app.cursor = body.map(|b| {
                        let p = at.unwrap_or(0);
                        Cursor { storage: b, owner: Owner::Body, anchor: p, caret: p, goal_x: None }
                    });
                    app.canvas.follow_caret = true;
                }
                // the body: nothing selected (the Format inspector says so)
                Owner::Body => app.cursor = None,
            }
        }
        _ => {}
    }
    let _ = ctx;
}
