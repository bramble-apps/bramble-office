//! Links (Format > Add Link, Mac N29 A): on an object (TSD.DrawableArchive.hyperlink_url) or
//! on text in a text box or shape (a HyperlinkFieldArchive over the text, kn_model::links).
//! The popover is Numbers': Display, Link to (Webpage / Email / Phone Number / Sheet), the
//! address, Remove and Open Link / Compose Email / Go to Sheet. Linked objects show the green
//! link badge; Edit > Substitutions > Smart Web and Email Links links addresses as they're typed.

use egui::{Color32, Pos2, Rect, Stroke};
use kn_ui::theme;
use nm_core::kn_format::ObjId;

use crate::app::App;

pub const KINDS: [&str; 4] = ["Webpage", "Email", "Phone Number", "Sheet"];

/// The open link popover.
#[derive(Clone, Debug)]
pub struct LinkEdit {
    /// The object linked (or holding the linked text).
    pub obj: ObjId,
    /// Linked text: (text storage, start, end) in stored bytes.
    pub text: Option<(ObjId, usize, usize)>,
    pub kind: String,
    pub addr: String,
    pub subject: String,
    /// The linked text (text links only).
    pub display: String,
    /// Sheet links: the sheet index.
    pub sheet: usize,
    /// A link is already there (Remove shows).
    pub existing: bool,
    /// Where the popover opens (under the object or text).
    pub at: Option<Pos2>,
}

/// A stored URL as (kind, address, subject, sheet UUID).
pub fn parse(url: &str) -> (&'static str, String, String, Option<String>) {
    if let Some(id) = url.strip_prefix("?sheetid=") {
        return ("Sheet", String::new(), String::new(), Some(id.to_string()));
    }
    if let Some(m) = url.strip_prefix("mailto:") {
        let (to, q) = m.split_once('?').unwrap_or((m, ""));
        let subject = q.split('&').find_map(|p| p.strip_prefix("subject=")).map(unescape).unwrap_or_default();
        return ("Email", to.to_string(), subject, None);
    }
    if let Some(t) = url.strip_prefix("tel:") {
        return ("Phone Number", t.to_string(), String::new(), None);
    }
    ("Webpage", url.to_string(), String::new(), None)
}

fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() && s.is_char_boundary(i + 3) {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn escape(s: &str) -> String {
    s.bytes().map(|c| if c.is_ascii_alphanumeric() || b"-._~".contains(&c) { (c as char).to_string() } else { format!("%{c:02X}") }).collect()
}

/// The URL Numbers stores for an address typed as `kind`.
pub fn url_of(kind: &str, addr: &str, subject: &str) -> String {
    let t = addr.trim();
    match kind {
        "Email" => {
            let to = t.trim_start_matches("mailto:");
            if subject.trim().is_empty() { format!("mailto:{to}") } else { format!("mailto:{to}?subject={}", escape(subject.trim())) }
        }
        "Phone Number" => format!("tel:{}", t.trim_start_matches("tel:").replace(' ', "")),
        _ if t.contains("://") => t.to_string(),
        _ => format!("https://{t}"),
    }
}

/// A word typed that Smart Web and Email Links (and Smart Phone Number Links) turn into a link:
/// its URL. ("www.apple.com" -> "http://www.apple.com", as the Mac stores it.)
pub fn smart_url(word: &str, phone: bool) -> Option<String> {
    let w = word.trim_end_matches(['.', ',', ';', ':', '!', '?', ')', '"', '\u{201d}', '\'']);
    let lower = w.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return (w.len() > 8).then(|| w.to_string());
    }
    if lower.starts_with("www.") && w[4..].contains('.') && !w.ends_with('.') {
        return Some(format!("http://{w}"));
    }
    if let Some((user, host)) = w.split_once('@') {
        let ok = !user.is_empty() && host.contains('.') && !host.starts_with('.') && !host.ends_with('.') && !host.contains('@') && w.chars().all(|c| c.is_alphanumeric() || "._%+-@".contains(c));
        return ok.then(|| format!("mailto:{w}"));
    }
    if phone {
        let digits = w.chars().filter(|c| c.is_ascii_digit()).count();
        if digits >= 7 && w.chars().all(|c| c.is_ascii_digit() || "+-(). ".contains(c)) && w.chars().next().is_some_and(|c| c.is_ascii_digit() || c == '+' || c == '(') {
            return Some(format!("tel:{}", w.replace([' ', '(', ')', '.'], "")));
        }
    }
    None
}

/// The text link around the caret of the text being edited: (start, end, url).
pub fn link_at_caret(app: &App) -> Option<(usize, usize, String)> {
    let ed = app.canvas.editing.as_ref()?;
    let (a, b) = ed.sel();
    let d = app.doc()?;
    d.text_links(ed.storage).into_iter().find(|l| if a == b { l.0 <= a && a <= l.1 && l.0 < l.1 } else { l.0 < b && a < l.1 })
}

fn sheet_uuid(app: &App, i: usize) -> Option<String> {
    let id = app.sheets().get(i)?.id;
    app.doc()?.file.object_uuid(id)
}

fn sheet_index(app: &App, uuid: &str) -> Option<usize> {
    let sheets = app.sheets();
    let d = app.doc()?;
    sheets.iter().position(|s| d.file.object_uuid(s.id).is_some_and(|u| u.eq_ignore_ascii_case(uuid)))
}

/// An object's link.
pub fn object_link(app: &App, id: ObjId) -> Option<String> {
    app.doc()?.file.obj(id)?.find_super("TSD.DrawableArchive")?.str("hyperlink_url").map(str::to_string)
}

/// Format > Add Link > kind: the popover for the selected text (while editing) or object.
pub fn open(app: &mut App, kind: &str) {
    let (obj, text, cur) = if let Some(ed) = app.canvas.editing.as_ref() {
        let (obj, storage) = (ed.id, ed.storage);
        let (a, b) = ed.sel();
        match link_at_caret(app) {
            Some((la, lb, url)) => (obj, Some((storage, la, lb)), Some(url)),
            None => (obj, Some((storage, a, b)), None),
        }
    } else {
        let Some(o) = app.canvas.obj else { return };
        (o, None, object_link(app, o))
    };
    let existing = cur.is_some();
    let (k, addr, subject, sheet) = cur.as_deref().map(parse).unwrap_or((kind_static(kind), String::new(), String::new(), None));
    // the menu's kind wins for a new link; an existing one opens as it is
    let k = if existing { k } else { kind_static(kind) };
    let display = text.and_then(|(s, a, b)| app.doc().map(|d| d.storage_text(s)).and_then(|t| t.get(a..b).map(str::to_string))).unwrap_or_default();
    let sheet = sheet.and_then(|u| sheet_index(app, &u)).unwrap_or_else(|| if app.sheet == 0 { 1 } else { 0 }.min(app.sheets().len().saturating_sub(1)));
    app.link_edit = Some(LinkEdit { obj, text, kind: k.to_string(), addr, subject, display, sheet, existing, at: None });
}

fn kind_static(kind: &str) -> &'static str {
    KINDS.iter().find(|k| **k == kind).copied().unwrap_or("Webpage")
}

/// Format > Remove Link: the text link at the caret, else the selected objects' links.
pub fn remove(app: &mut App) {
    if let Some((a, _, _)) = link_at_caret(app) {
        let storage = app.canvas.editing.as_ref().map(|e| e.storage).unwrap();
        remove_text(app, storage, a);
        return;
    }
    let ids = app.selected_objs();
    app.edit("Remove Link", |d, h| ids.iter().for_each(|i| d.set_hyperlink(h, *i, None)));
}

fn remove_text(app: &mut App, storage: ObjId, at: usize) {
    if app.canvas.editing.is_some() {
        if let Some(w) = app.wb.as_mut() {
            w.doc.remove_text_link(&mut app.hist, storage, at);
        }
        text_changed(app);
    } else {
        app.edit("Remove Link", |d, h| {
            d.remove_text_link(h, storage, at);
        });
    }
}

fn text_changed(app: &mut App) {
    if let Some(ed) = app.canvas.editing.as_mut() {
        ed.version += 1;
    }
    app.changed();
}

pub fn enabled(app: &App, name: &str) -> bool {
    match name {
        "Remove Link" => link_at_caret(app).is_some() || (app.canvas.editing.is_none() && app.canvas.obj.is_some_and(|o| object_link(app, o).is_some())),
        _ => app.canvas.editing.is_some() || app.canvas.obj.is_some_and(|o| !crate::menus::is_table(app, o)),
    }
}

/// Follow a stored link: a sheet link goes to the sheet, others open in the desktop's app.
pub fn follow(app: &mut App, url: &str) {
    if let Some(u) = url.strip_prefix("?sheetid=") {
        match sheet_index(app, u) {
            Some(i) => {
                crate::canvas::commit(app);
                app.sheet = i;
                app.canvas.reset();
                app.sel = None;
                app.refresh();
            }
            None => app.notify("The linked sheet isn't in this spreadsheet any more"),
        }
        return;
    }
    let url = if url.contains("://") || url.starts_with("mailto:") || url.starts_with("tel:") { url.to_string() } else { format!("https://{url}") };
    std::thread::spawn(move || {
        let _ = kn_ui::open_command(&url).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
    });
}

/// The link popover.
pub fn window(app: &mut App, ctx: &egui::Context) {
    let Some(mut le) = app.link_edit.clone() else { return };
    let sheets: Vec<String> = app.sheets().iter().map(|s| s.name.clone()).collect();
    let mut open = true;
    // None: still open; Some(Some(url)): apply; Some(None): remove
    let mut done: Option<Option<String>> = None;
    let mut close = false;
    let mut follow_url: Option<String> = None;
    let mut w = egui::Window::new("Link").title_bar(false).collapsible(false).resizable(false).open(&mut open);
    w = match le.at {
        Some(p) => w.fixed_pos(p),
        None => w.pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()),
    };
    w.show(ctx, |ui| {
        ui.set_width(300.0);
        egui::Grid::new("link-pop").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
            if le.text.is_some() {
                ui.label("Display:");
                ui.add(egui::TextEdit::singleline(&mut le.display).desired_width(200.0));
                ui.end_row();
            }
            ui.label("Link to:");
            theme::combo("link-kind").selected_text(le.kind.clone()).width(200.0).show_ui(ui, |ui| {
                for k in KINDS {
                    ui.selectable_value(&mut le.kind, k.to_string(), k);
                }
            });
            ui.end_row();
            match le.kind.as_str() {
                "Sheet" => {
                    ui.label("Sheet:");
                    theme::combo("link-sheet").selected_text(sheets.get(le.sheet).cloned().unwrap_or_default()).width(200.0).show_ui(ui, |ui| {
                        for (i, n) in sheets.iter().enumerate() {
                            ui.selectable_value(&mut le.sheet, i, n);
                        }
                    });
                    ui.end_row();
                }
                k => {
                    ui.label(if k == "Email" { "To:" } else if k == "Phone Number" { "Number:" } else { "Link:" });
                    let hint = match k {
                        "Email" => "name@example.com",
                        "Phone Number" => "+1 555 123 4567",
                        _ => "www.example.com",
                    };
                    let r = ui.add(egui::TextEdit::singleline(&mut le.addr).hint_text(hint).desired_width(200.0));
                    if !le.existing && le.addr.is_empty() && !r.has_focus() && ui.memory(|m| m.focused().is_none()) {
                        r.request_focus();
                    }
                    ui.end_row();
                    if k == "Email" {
                        ui.label("Subject:");
                        ui.add(egui::TextEdit::singleline(&mut le.subject).desired_width(200.0));
                        ui.end_row();
                    }
                }
            }
        });
        let ready = le.kind == "Sheet" || !le.addr.trim().is_empty();
        if ui.input(|i| i.key_pressed(egui::Key::Enter)) && ready {
            done = Some(Some(String::new()));
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            close = true;
        }
        ui.add_space(4.0);
        ui.separator();
        ui.columns(2, |c| {
            if le.existing {
                if c[0].add_sized([c[0].available_width(), 24.0], egui::Button::new("Remove")).clicked() {
                    done = Some(None);
                }
            } else if c[0].add_sized([c[0].available_width(), 24.0], egui::Button::new("Cancel")).clicked() {
                close = true;
            }
            let open_label = match le.kind.as_str() {
                "Sheet" => "Go to Sheet",
                "Email" => "Compose Email",
                "Phone Number" => "Call",
                _ => if le.existing { "Open Link" } else { "Add Link" },
            };
            let go = c[1].add_enabled_ui(ready, |ui| ui.add_sized([ui.available_width(), 24.0], egui::Button::new(open_label))).inner.clicked();
            if go {
                if le.existing || le.kind != "Webpage" {
                    follow_url = Some(String::new());
                }
                done = Some(Some(String::new()));
            }
        });
    });
    let url = match le.kind.as_str() {
        "Sheet" => sheet_uuid(app, le.sheet).map(|u| format!("?sheetid={u}")),
        k => Some(url_of(k, &le.addr, &le.subject)),
    };
    if !open || close {
        app.link_edit = None;
        return;
    }
    match done {
        None => app.link_edit = Some(le),
        Some(None) => {
            app.link_edit = None;
            match le.text {
                Some((s, a, _)) => remove_text(app, s, a),
                None => {
                    let o = le.obj;
                    app.edit("Remove Link", |d, h| d.set_hyperlink(h, o, None));
                }
            }
        }
        Some(Some(_)) => {
            app.link_edit = None;
            let Some(url) = url else { return };
            apply(app, &le, &url);
            if follow_url.is_some() {
                follow(app, &url);
            }
        }
    }
}

fn apply(app: &mut App, le: &LinkEdit, url: &str) {
    match le.text {
        Some((storage, a, b)) => {
            let display = if le.display.is_empty() { if le.kind == "Sheet" { app.sheets().get(le.sheet).map(|s| s.name.clone()).unwrap_or_default() } else { le.addr.trim().to_string() } } else { le.display.clone() };
            let editing = app.canvas.editing.is_some();
            let url = url.to_string();
            let run = |d: &mut nm_core::kn_model::Document, h: &mut nm_core::kn_model::edit::History| {
                let raw = d.storage_text(storage);
                let (a, b) = (a.min(raw.len()), b.min(raw.len()));
                let b = if raw.get(a..b) != Some(display.as_str()) {
                    d.replace_text(h, storage, a, b, &display);
                    a + display.len()
                } else {
                    b
                };
                d.add_text_link(h, storage, a, b, &url);
                b
            };
            if editing {
                let Some(w) = app.wb.as_mut() else { return };
                let end = run(&mut w.doc, &mut app.hist);
                if let Some(ed) = app.canvas.editing.as_mut() {
                    ed.cursor = end;
                    ed.anchor = end;
                }
                text_changed(app);
            } else {
                app.edit("Add Link", |d, h| {
                    run(d, h);
                });
            }
        }
        None => {
            let o = le.obj;
            app.edit("Add Link", |d, h| d.set_hyperlink(h, o, Some(url)));
        }
    }
}

/// Smart Web and Email Links: the word that just ended before byte `end` of the edited text,
/// linked when it's an address. Called after a space, return or punctuation is typed.
pub fn smart_link_typed(app: &mut App, end: usize) {
    if !app.settings.smart_links && !app.settings.smart_phone_links {
        return;
    }
    let Some(storage) = app.canvas.editing.as_ref().map(|e| e.storage) else { return };
    let Some(text) = app.doc().map(|d| d.storage_text(storage)) else { return };
    let end = end.min(text.len());
    let start = text[..end].rfind(|c: char| c.is_whitespace() || c == '\u{fffc}').map(|i| i + text[i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1)).unwrap_or(0);
    let word = &text[start..end];
    let Some(url) = smart_url(word, app.settings.smart_phone_links) else { return };
    if !app.settings.smart_links && !url.starts_with("tel:") {
        return;
    }
    let trimmed = word.trim_end_matches(['.', ',', ';', ':', '!', '?', ')', '"', '\u{201d}', '\'']);
    let b = start + trimmed.len();
    let Some(d) = app.doc() else { return };
    if d.text_links(storage).iter().any(|l| l.0 < b && start < l.1) {
        return;
    }
    if let Some(w) = app.wb.as_mut() {
        w.doc.add_text_link(&mut app.hist, storage, start, b, &url);
    }
    text_changed(app);
}

/// The green link badge at an object's bottom-right corner (screen rect `r`); true when clicked.
pub fn badge(ui: &mut egui::Ui, r: Rect, id: ObjId) -> bool {
    let c = Pos2::new(r.max.x - 16.0, r.max.y - 16.0);
    let hit = Rect::from_center_size(c, egui::vec2(20.0, 20.0));
    let resp = ui.interact(hit, ui.scope_id().with(("link-badge", id)), egui::Sense::click());
    let p = ui.painter();
    p.circle_filled(c, 11.0, Color32::WHITE);
    p.circle_filled(c, 9.5, if resp.hovered() { Color32::from_rgb(0x3f, 0xb3, 0x4f) } else { Color32::from_rgb(0x52, 0xc4, 0x5f) });
    // a curved "forward" arrow
    let s = Stroke::new(1.8, Color32::WHITE);
    let pts: Vec<Pos2> = (0..=8).map(|i| {
        let t = std::f32::consts::PI * (1.0 - i as f32 / 8.0 * 0.75);
        Pos2::new(c.x + 1.5 + 4.5 * t.cos(), c.y + 3.5 - 4.5 * t.sin())
    }).collect();
    p.add(egui::Shape::line(pts, s));
    let tip = Pos2::new(c.x + 4.6, c.y - 1.0);
    p.line_segment([tip, tip + egui::vec2(-3.2, -2.6)], s);
    p.line_segment([tip, tip + egui::vec2(-3.4, 2.4)], s);
    resp.on_hover_text("Link").clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smart_urls() {
        assert_eq!(smart_url("www.apple.com", false).as_deref(), Some("http://www.apple.com"));
        assert_eq!(smart_url("www.apple.com.", false).as_deref(), Some("http://www.apple.com"));
        assert_eq!(smart_url("test@example.com", false).as_deref(), Some("mailto:test@example.com"));
        assert_eq!(smart_url("https://x.org/a", false).as_deref(), Some("https://x.org/a"));
        assert_eq!(smart_url("hello", false), None);
        assert_eq!(smart_url("me@home", false), None);
        assert_eq!(smart_url("555-123-4567", false), None);
        assert_eq!(smart_url("555-123-4567", true).as_deref(), Some("tel:555-123-4567"));
    }

    #[test]
    fn urls_round_trip() {
        assert_eq!(url_of("Email", "a@b.com", "Hi there"), "mailto:a@b.com?subject=Hi%20there");
        assert_eq!(parse("mailto:a@b.com?subject=Hi%20there"), ("Email", "a@b.com".into(), "Hi there".into(), None));
        assert_eq!(url_of("Webpage", "www.apple.com", ""), "https://www.apple.com");
        assert_eq!(parse("?sheetid=ABC").0, "Sheet");
        assert_eq!(url_of("Phone Number", "+1 555 1234", ""), "tel:+15551234");
    }
}
