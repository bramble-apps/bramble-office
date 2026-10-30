//! In-place text editing on the canvas: the text is drawn by the real renderer (true fonts and
//! formatting) and every keystroke edits the document through `Document::replace_text`, which
//! keeps run and paragraph formatting. One editing session is one undo step.

use std::time::Instant;

use egui::{Color32, Event, Key, Pos2, Stroke};
use kn_format::ObjId;
use kn_model::{DrawableKind, Geometry, Point, SlideScene};
use kn_render::text::{caret, hit_index, TextLayout};

use crate::app::App;
use crate::theme;

pub struct Editing {
    pub id: ObjId,
    pub storage: ObjId,
    pub cursor: usize,
    pub anchor: usize,
    /// Bumped on every text change (drives re-rendering of the edited object).
    pub version: u64,
    blink: Instant,
    /// Scene rebuilt after the latest change (the app's scene cache is per slide revision).
    pub scene: Option<SlideScene>,
    /// Misspelled ranges for `spell_version`.
    spelling: Vec<(usize, usize)>,
    spell_version: Option<u64>,
    /// Right-click spelling menu: (range, suggestions, screen position).
    pub spell_menu: Option<(usize, usize, Vec<String>, Pos2)>,
}

impl Editing {
    fn sel(&self) -> (usize, usize) {
        (self.cursor.min(self.anchor), self.cursor.max(self.anchor))
    }
}

/// The renderer's layout with byte offsets in the stored text: fields (one U+FFFC stored, their
/// value shown) become a single unit the caret steps over.
pub fn stored_layout(tl: Option<(TextLayout, Geometry)>, d: &kn_model::Drawable) -> Option<(TextLayout, Geometry)> {
    let (mut tl, g) = tl?;
    if let DrawableKind::Shape(s) = &d.kind {
        if let Some(tb) = s.text.as_ref().filter(|t| !t.fields.is_empty()) {
            for gl in &mut tl.glyphs {
                gl.start = tb.stored_offset(gl.start);
                gl.end = tb.stored_offset(gl.end);
            }
            for l in &mut tl.lines {
                l.start = tb.stored_offset(l.start);
                l.end = tb.stored_offset(l.end);
            }
        }
    }
    Some((tl, g))
}

fn layout_of(app: &mut App, scene: &SlideScene, id: ObjId) -> Option<(TextLayout, Geometry)> {
    let d = scene.drawables.iter().find(|d| d.id == id)?;
    stored_layout(app.renderer.text_layout(d), d)
}

/// Frame-local text coordinates of a slide point.
fn local(g: &Geometry, p: Point) -> Point {
    // text is not flipped with its shape, so undo rotation only
    let mut g2 = *g;
    g2.hflip = false;
    g2.vflip = false;
    g2.to_local(p)
}

/// Start editing drawable `id` with the caret nearest to slide point `at` (None = select all).
pub fn begin(app: &mut App, scene: &SlideScene, id: ObjId, at: Option<Point>) -> bool {
    let Some(d) = scene.drawables.iter().find(|d| d.id == id) else { return false };
    let DrawableKind::Shape(s) = &d.kind else { return false };
    let Some(tb) = &s.text else { return false };
    let storage = tb.storage;
    let Some(doc) = app.doc.as_mut() else { return false };
    let raw = doc.storage_text(storage);
    doc.begin(&mut app.hist, "Typing");
    let (cursor, anchor) = match at.and_then(|p| layout_of(app, scene, id).map(|(tl, g)| (tl, g, p))) {
        Some((tl, g, p)) => {
            let l = local(&g, p);
            let i = hit_index(&tl, l.x, l.y).min(raw.len());
            (i, i)
        }
        None => (raw.len(), 0),
    };
    app.canvas.editing = Some(Editing { id, storage, cursor, anchor, version: 0, blink: Instant::now(), scene: None, spelling: Vec::new(), spell_version: None, spell_menu: None });
    true
}

/// Finish editing: close the undo step and refresh the slide.
pub fn commit(app: &mut App) {
    let Some(ed) = app.canvas.editing.take() else { return };
    if let Some(doc) = app.doc.as_mut() {
        doc.end(&mut app.hist);
    }
    if ed.version > 0 {
        if let Some(s) = app.current_slide().map(|s| s.slide) {
            app.bump(s);
        }
    }
}

fn prev_boundary(s: &str, i: usize) -> usize {
    s[..i].char_indices().last().map(|(b, _)| b).unwrap_or(0)
}

fn next_boundary(s: &str, i: usize) -> usize {
    s[i..].chars().next().map(|c| i + c.len_utf8()).unwrap_or(s.len())
}

fn word_left(s: &str, i: usize) -> usize {
    let mut j = i;
    while j > 0 && s[..j].ends_with(char::is_whitespace) {
        j = prev_boundary(s, j);
    }
    while j > 0 && !s[..j].ends_with(char::is_whitespace) {
        j = prev_boundary(s, j);
    }
    j
}

fn word_right(s: &str, i: usize) -> usize {
    let mut j = i;
    while j < s.len() && s[j..].starts_with(char::is_whitespace) {
        j = next_boundary(s, j);
    }
    while j < s.len() && !s[j..].starts_with(char::is_whitespace) {
        j = next_boundary(s, j);
    }
    j
}

/// Apply keyboard input. Returns true if the text changed.
pub fn handle_keys(app: &mut App, ctx: &egui::Context, tl: Option<&TextLayout>) {
    let Some(storage) = app.canvas.editing.as_ref().map(|e| e.storage) else { return };
    let events = ctx.input(|i| i.events.clone());
    for ev in events {
        let Some(doc) = app.doc.as_mut() else { return };
        let text = doc.storage_text(storage);
        let Some(ed) = app.canvas.editing.as_mut() else { return };
        ed.cursor = ed.cursor.min(text.len());
        ed.anchor = ed.anchor.min(text.len());
        let (a, b) = ed.sel();
        let mut replace: Option<(usize, usize, String)> = None;
        match ev {
            Event::Text(t) => {
                let (back, ins) = smart_text(text[..a].chars().last(), &t, a == b, app.settings.smart_quotes, app.settings.smart_dashes);
                // a word just ended: text replacements, then spelling autocorrection
                let ends_word = a == b && back == 0 && t.chars().all(|c| c.is_whitespace() || ".,;:!?)".contains(c));
                match ends_word.then(|| word_fix(&text[..a], &app.settings)).flatten() {
                    Some((start, fixed)) => replace = Some((start, b, fixed + &ins)),
                    None => replace = Some((a - back, b, ins)),
                }
            }
            Event::Paste(t) => replace = Some((a, b, t.replace("\r\n", "\n").replace('\r', "\n"))),
            Event::Copy => {
                if a < b {
                    ctx.copy_text(text[a..b].to_string());
                }
            }
            Event::Cut => {
                if a < b {
                    ctx.copy_text(text[a..b].to_string());
                    replace = Some((a, b, String::new()));
                }
            }
            Event::Key { key, pressed: true, modifiers, .. } => {
                let shift = modifiers.shift;
                let word = modifiers.command || modifiers.alt;
                let mut moved = None;
                match key {
                    Key::Escape => {
                        commit(app);
                        return;
                    }
                    Key::Enter => {
                        replace = Some((a, b, if shift { "\u{2028}".into() } else { "\n".into() }));
                    }
                    Key::Tab => {
                        // in a bulleted paragraph Tab / Shift+Tab change the list level (Keynote)
                        let bulleted = ed
                            .scene
                            .as_ref()
                            .and_then(|sc| sc.drawables.iter().find(|d| d.id == ed.id))
                            .and_then(|d| match &d.kind {
                                DrawableKind::Shape(s) => s.text.as_ref(),
                                _ => None,
                            })
                            .and_then(|t| t.paragraphs.iter().find(|p| p.start <= a && a <= p.end))
                            .is_some_and(|p| p.label.is_some());
                        if bulleted || shift {
                            if let Some(doc) = app.doc.as_mut() {
                                doc.change_list_level(&mut app.hist, storage, a, b, if shift { -1 } else { 1 });
                            }
                            let ed = app.canvas.editing.as_mut().unwrap();
                            ed.version += 1;
                            ed.scene = None;
                            continue;
                        }
                        replace = Some((a, b, "\t".into()));
                    }
                    Key::Backspace => {
                        if a < b {
                            replace = Some((a, b, String::new()));
                        } else if a > 0 {
                            let p = if word { word_left(&text, a) } else { prev_boundary(&text, a) };
                            replace = Some((p, a, String::new()));
                        }
                    }
                    Key::Delete => {
                        if a < b {
                            replace = Some((a, b, String::new()));
                        } else if a < text.len() {
                            let n = if word { word_right(&text, a) } else { next_boundary(&text, a) };
                            replace = Some((a, n, String::new()));
                        }
                    }
                    Key::ArrowLeft => {
                        moved = Some(if !shift && a < b { a } else if word { word_left(&text, ed.cursor) } else { prev_boundary(&text, ed.cursor) })
                    }
                    Key::ArrowRight => {
                        moved = Some(if !shift && a < b { b } else if word { word_right(&text, ed.cursor) } else { next_boundary(&text, ed.cursor) })
                    }
                    Key::ArrowUp | Key::ArrowDown => {
                        if let Some(tl) = tl {
                            if let Some((x, top, h)) = caret(tl, ed.cursor) {
                                let y = if key == Key::ArrowUp { top - h * 0.5 } else { top + h * 1.5 };
                                moved = Some(hit_index(tl, x, y).min(text.len()));
                            }
                        }
                    }
                    Key::Home => {
                        moved = Some(tl.and_then(|tl| tl.lines.iter().rev().find(|l| l.start <= ed.cursor)).map(|l| l.start).unwrap_or(0));
                    }
                    Key::End => {
                        moved = Some(tl.and_then(|tl| tl.lines.iter().rev().find(|l| l.start <= ed.cursor)).map(|l| l.end).unwrap_or(text.len()));
                    }
                    Key::B | Key::I | Key::U if modifiers.command => {
                        // toggle based on the style at the start of the selection
                        let style = ed
                            .scene
                            .as_ref()
                            .and_then(|sc| sc.drawables.iter().find(|d| d.id == ed.id))
                            .and_then(|d| match &d.kind {
                                DrawableKind::Shape(s) => s.text.as_ref(),
                                _ => None,
                            })
                            .and_then(|t| t.paragraphs.iter().flat_map(|p| p.runs.iter()).find(|r| r.start <= a && a < r.end.max(r.start + 1)).map(|r| r.style.clone()));
                        let st = style.unwrap_or_default();
                        let prop = match key {
                            Key::B => kn_model::format::TextProp::Bold(!st.bold),
                            Key::I => kn_model::format::TextProp::Italic(!st.italic),
                            _ => kn_model::format::TextProp::Underline(!st.underline),
                        };
                        if a < b {
                            crate::editing::set_text(app, "Format Text", prop);
                        }
                        continue;
                    }
                    Key::A if modifiers.command => {
                        ed.anchor = 0;
                        ed.cursor = text.len();
                    }
                    _ => {}
                }
                if let Some(m) = moved {
                    ed.cursor = m.min(text.len());
                    if !shift {
                        ed.anchor = ed.cursor;
                    }
                    ed.blink = Instant::now();
                }
            }
            _ => {}
        }
        if let Some((s, e, ins)) = replace {
            let Some(doc) = app.doc.as_mut() else { return };
            doc.replace_text(&mut app.hist, storage, s, e, &ins);
            let ed = app.canvas.editing.as_mut().unwrap();
            ed.cursor = s + ins.len();
            ed.anchor = ed.cursor;
            ed.version += 1;
            ed.scene = None;
            ed.blink = Instant::now();
        }
    }
}

/// Typed text after Smart Quotes / Smart Dashes: (bytes to replace before the caret, text).
/// `"` and `'` become curly quotes (opening after a space or bracket), `--` becomes an em dash.
/// Text replacements (kn-ui's).
pub use kn_ui::typing::REPLACEMENTS;
use kn_ui::typing::smart_text;

/// The word before the caret and what it should become (kn-ui's, with Keydeck's settings).
fn word_fix(before: &str, s: &crate::settings::Settings) -> Option<(usize, String)> {
    crate::spell::ensure();
    kn_ui::typing::word_fix(before, s.text_replacement, s.auto_correct && s.spell_check)
}

/// Type `ins` at the caret of the text being edited (replacing the selection), e.g. Insert >
/// Line Break. Returns false when no text is being edited.
pub fn insert_at_caret(app: &mut App, ins: &str) -> bool {
    let Some((storage, a, b)) = app.canvas.editing.as_ref().map(|e| (e.storage, e.sel().0, e.sel().1)) else { return false };
    let Some(doc) = app.doc.as_mut() else { return false };
    let len = doc.storage_text(storage).len();
    let (a, b) = (a.min(len), b.min(len));
    doc.replace_text(&mut app.hist, storage, a, b, ins);
    let Some(ed) = app.canvas.editing.as_mut() else { return false };
    ed.cursor = a + ins.len();
    ed.anchor = ed.cursor;
    ed.version += 1;
    ed.scene = None;
    ed.blink = Instant::now();
    true
}

/// Misspelled ranges of the edited text (recomputed when the text changes).
fn spelling(app: &mut App) -> Vec<(usize, usize)> {
    if !app.settings.spell_check {
        return Vec::new();
    }
    let Some(ed) = app.canvas.editing.as_ref() else { return Vec::new() };
    let loaded = crate::spell::dicts().is_some();
    if ed.spell_version != Some(ed.version) || !loaded {
        let text = app.doc.as_ref().map(|d| d.storage_text(ed.storage)).unwrap_or_default();
        let ranges = crate::spell::misspelled(&text);
        let ed = app.canvas.editing.as_mut().unwrap();
        ed.spelling = ranges;
        if loaded {
            ed.spell_version = Some(ed.version);
        }
    }
    app.canvas.editing.as_ref().map(|e| e.spelling.clone()).unwrap_or_default()
}

/// Red squiggles under misspelled words.
pub fn draw_spelling(app: &mut App, painter: &egui::Painter, tl: &TextLayout, g: &Geometry, to_screen: impl Fn(Point) -> Pos2) {
    let mut g2 = *g;
    g2.hflip = false;
    g2.vflip = false;
    let grammar: Vec<(usize, usize)> = if app.settings.grammar && app.settings.spell_check {
        let text = app.canvas.editing.as_ref().and_then(|ed| app.doc.as_ref().map(|d| d.storage_text(ed.storage))).unwrap_or_default();
        crate::spell::grammar(&text)
    } else {
        Vec::new()
    };
    let marks: Vec<(usize, usize, Color32)> =
        spelling(app).into_iter().map(|(s, e)| (s, e, Color32::from_rgb(0xff, 0x3b, 0x30))).chain(grammar.into_iter().map(|(s, e)| (s, e, Color32::from_rgb(0x34, 0xc7, 0x59)))).collect();
    for (s, e, col) in marks {
        let mut by_line: Vec<(usize, f32, f32, f32, f32)> = Vec::new();
        for gl in tl.glyphs.iter().filter(|gl| !gl.label && gl.start >= s && gl.end <= e && gl.end > gl.start) {
            let y = gl.y + gl.size * 0.14;
            match by_line.iter_mut().find(|l| l.0 == gl.line) {
                Some(l) => {
                    l.1 = l.1.min(gl.x);
                    l.2 = l.2.max(gl.x + gl.w);
                }
                None => by_line.push((gl.line, gl.x, gl.x + gl.w, y, gl.size)),
            }
        }
        for (_, x0, x1, y, size) in by_line {
            let amp = (size * 0.04).max(1.0);
            let step = amp * 2.5;
            let mut pts = Vec::new();
            let mut x = x0;
            let mut up = true;
            while x <= x1 {
                pts.push(to_screen(g2.to_slide(Point::new(x, y + if up { 0.0 } else { amp }))));
                up = !up;
                x += step;
            }
            if pts.len() > 1 {
                painter.add(egui::Shape::line(pts, Stroke::new(1.5, col)));
            }
        }
    }
}

/// Right-click inside the edited text: open spelling suggestions for the word under the pointer.
pub fn open_spell_menu(app: &mut App, tl: &TextLayout, g: &Geometry, p: Point, screen: Pos2) {
    let l = local(g, p);
    let i = hit_index(tl, l.x, l.y);
    let ranges = spelling(app);
    let Some(&(s, e)) = ranges.iter().find(|(s, e)| *s <= i && i <= *e) else { return };
    let text = app.doc.as_ref().map(|d| d.storage_text(app.canvas.editing.as_ref().unwrap().storage)).unwrap_or_default();
    let sug = crate::spell::suggestions(&text[s..e]);
    if let Some(ed) = app.canvas.editing.as_mut() {
        ed.spell_menu = Some((s, e, sug, screen));
    }
}

/// Show the spelling menu (if open); replaces the word on click.
pub fn spell_menu_ui(app: &mut App, ctx: &egui::Context) {
    let Some((s, e, sug, pos)) = app.canvas.editing.as_ref().and_then(|ed| ed.spell_menu.clone()) else { return };
    let mut pick: Option<String> = None;
    let mut close = false;
    let (mut ignore_word, mut learn_word) = (false, false);
    let area = egui::Area::new(egui::Id::unique("spell-menu")).fixed_pos(pos).order(egui::Order::Foreground).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            if sug.is_empty() {
                ui.label(egui::RichText::new("No Guesses Found").italics());
            }
            for w in &sug {
                if ui.button(w).clicked() {
                    pick = Some(w.clone());
                }
            }
            ui.separator();
            if ui.button("Ignore Spelling").clicked() {
                ignore_word = true;
                close = true;
            }
            if ui.button("Learn Spelling").clicked() {
                learn_word = true;
                close = true;
            }
        });
    });
    if area.response.clicked_elsewhere() {
        close = true;
    }
    if let Some(w) = pick {
        let storage = app.canvas.editing.as_ref().unwrap().storage;
        if let Some(doc) = app.doc.as_mut() {
            doc.replace_text(&mut app.hist, storage, s, e, &w);
        }
        let ed = app.canvas.editing.as_mut().unwrap();
        ed.cursor = s + w.len();
        ed.anchor = ed.cursor;
        ed.version += 1;
        ed.scene = None;
        close = true;
    }
    if ignore_word || learn_word {
        let storage = app.canvas.editing.as_ref().unwrap().storage;
        let text = app.doc.as_ref().map(|d| d.storage_text(storage)).unwrap_or_default();
        if let Some(w) = text.get(s..e) {
            if learn_word {
                crate::spell::learn(w);
            } else {
                crate::spell::ignore(w);
            }
        }
        if let Some(ed) = app.canvas.editing.as_mut() {
            ed.spell_version = None;
        }
    }
    if close {
        if let Some(ed) = app.canvas.editing.as_mut() {
            ed.spell_menu = None;
        }
    }
}

/// Draw the selection highlight and caret over the edited object.
pub fn draw_overlay(app: &App, painter: &egui::Painter, tl: &TextLayout, g: &Geometry, to_screen: impl Fn(Point) -> Pos2) {
    let Some(ed) = app.canvas.editing.as_ref() else { return };
    let mut g2 = *g;
    g2.hflip = false;
    g2.vflip = false;
    let map = |x: f32, y: f32| to_screen(g2.to_slide(Point::new(x, y)));
    let (a, b) = ed.sel();
    if a < b {
        for (li, line) in tl.lines.iter().enumerate() {
            if line.end < a || line.start > b || (line.start == line.end && !(a..b).contains(&line.start)) {
                continue;
            }
            let x0 = caret(tl, a.max(line.start)).filter(|_| a >= line.start).map(|c| c.0).unwrap_or(line.x);
            let x1 = if b <= line.end { caret(tl, b).map(|c| c.0).unwrap_or(line.x + line.width) } else { line.x + line.width + 6.0 };
            let _ = li;
            let pts = vec![map(x0, line.top), map(x1, line.top), map(x1, line.top + line.height), map(x0, line.top + line.height)];
            painter.add(egui::Shape::convex_polygon(pts, Color32::from_rgba_unmultiplied(0x3a, 0x82, 0xf7, 90), Stroke::NONE));
        }
    }
    let on = (ed.blink.elapsed().as_millis() / 530) % 2 == 0;
    if on {
        if let Some((x, top, h)) = caret(tl, ed.cursor) {
            painter.line_segment([map(x, top), map(x, top + h)], Stroke::new(2.0, theme::ACCENT()));
        }
    }
    // dashed-looking frame like Keynote's text editing outline
    let corners = [map(0.0, 0.0), map(g.w, 0.0), map(g.w, g.h), map(0.0, g.h)];
    for i in 0..4 {
        painter.line_segment([corners[i], corners[(i + 1) % 4]], Stroke::new(1.0, theme::ACCENT().gamma_multiply(0.6)));
    }
}

/// Move the caret / extend the selection to a slide point (mouse).
pub fn point_to(app: &mut App, tl: &TextLayout, g: &Geometry, p: Point, extend: bool) {
    let l = local(g, p);
    let i = hit_index(tl, l.x, l.y);
    if let Some(ed) = app.canvas.editing.as_mut() {
        ed.cursor = i;
        if !extend {
            ed.anchor = i;
        }
        ed.blink = Instant::now();
    }
}

pub fn contains(g: &Geometry, p: Point) -> bool {
    let l = local(g, p);
    l.x >= -4.0 && l.y >= -4.0 && l.x <= g.w + 4.0 && l.y <= g.h + 4.0
}
