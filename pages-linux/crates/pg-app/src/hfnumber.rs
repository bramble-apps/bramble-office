//! Page numbers, as Pages does them: hovering the header / footer band only outlines its three
//! fields (canvas.rs); once a click puts the caret in a field, an "Insert Page Number" button
//! shows below the header (above the footer), unless that page's header / footer already has a
//! page number (then Insert > Page Number, as in Pages). Its menu, the Insert menu's Page Number
//! and the field's right-click menu insert "1", "Page 1", "1 of 3" or "Page 1 of 3" at the caret
//! and set the number style (1 2 3, i ii iii, I II III, a b c, A B C).

use egui::{vec2, Color32, Rect, Sense, Stroke};
use kn_ui::theme;

use pg_core::kn_format::ObjId;

use crate::app::{App, Cursor, Owner};

#[derive(Clone, Copy)]
struct Target {
    page: usize,
    storage: ObjId,
    owner: Owner,
}

/// Where page numbers go: the text with the caret (a header / footer field, or any text).
fn target(app: &App) -> Option<Target> {
    let c = app.cursor?;
    let page = match c.owner {
        Owner::HeaderFooter { page } | Owner::Footnote { page } => page,
        _ => app.canvas.page_in_view,
    };
    Some(Target { page, storage: c.storage, owner: c.owner })
}

/// The Page Number submenu's items (the Insert menu and a header / footer's right-click menu):
/// "1", "Page 1", "1 of 3", "Page 1 of 3" at the caret, then the number style of the page
/// numbers in that text (1 2 3, i ii iii, I II III, a b c, A B C).
pub fn menu_items(app: &mut App, ui: &mut egui::Ui) {
    let Some(t) = target(app) else {
        ui.add_enabled(false, egui::Button::new("Place the caret in text first"));
        return;
    };
    let (page_no, count) = app.layout.as_ref().map(|l| (l.pages.get(t.page).map(|p| p.number).unwrap_or(1), l.page_count())).unwrap_or((1, 1));
    let current = app.doc().and_then(|d| pg_core::edit::page_field_format(d, t.storage));
    let fmt = |n: usize| pg_core::kn_model::text::format_number(n as u32, current.unwrap_or(0)).trim_start_matches('(').trim_end_matches(['.', ')']).to_string();
    let (n, c) = (fmt(page_no), fmt(count));
    for (k, label) in [n.clone(), format!("Page {n}"), format!("{n} of {c}"), format!("Page {n} of {c}")].into_iter().enumerate() {
        if ui.add(egui::Button::new(label).min_size(egui::vec2(180.0, 0.0))).clicked() {
            insert(app, t, k);
            ui.close();
        }
    }
    ui.separator();
    ui.menu_button("Number Format", |ui| {
        for (code, label) in [(0u64, "1, 2, 3"), (6, "i, ii, iii"), (3, "I, II, III"), (12, "a, b, c"), (9, "A, B, C")] {
            let on = current.unwrap_or(0) == code;
            if ui.add_enabled(current.is_some(), egui::Button::new(format!("{}  {label}", if on { "✓" } else { "   " }))).clicked() {
                app.edit("Page Number Format", |d, h| pg_core::edit::set_page_field_format(d, h, t.storage, code));
                ui.close();
            }
        }
    });
}

/// Insert "1" / "Page 1" / "1 of N" / "Page 1 of N" into the field: at the caret when it is in
/// that field, else at its end; the caret goes after it.
fn insert(app: &mut App, t: Target, kind: usize) {
    enum Part {
        Text(&'static str),
        Number,
        Count,
    }
    let parts: Vec<Part> = match kind {
        0 => vec![Part::Number],
        1 => vec![Part::Text("Page "), Part::Number],
        2 => vec![Part::Number, Part::Text(" of "), Part::Count],
        _ => vec![Part::Text("Page "), Part::Number, Part::Text(" of "), Part::Count],
    };
    let st = t.storage;
    let at = match app.cursor {
        Some(c) if c.storage == st => c.range().0,
        _ => app.doc().map(|d| d.storage_text(st).trim_end_matches(|c: char| pg_core::body::is_para_end(c)).len()).unwrap_or(0),
    };
    let end = app.edit("Insert Page Number", |d, h| {
        let mut pos = at;
        for p in &parts {
            match p {
                Part::Text(s) => {
                    pg_core::edit::replace(d, h, st, pos, pos, s);
                    pos += s.len();
                }
                Part::Number | Part::Count => {
                    pg_core::edit::insert_page_field(d, h, st, pos, matches!(p, Part::Count));
                    pos += '\u{FFFC}'.len_utf8();
                }
            }
        }
        pos
    });
    if let Some(pos) = end {
        app.cursor = Some(Cursor { storage: st, owner: t.owner, anchor: pos, caret: pos, goal_x: None });
    }
}

thread_local! {
    /// The button's menu is open (for this storage), and when it opened.
    static MENU: std::cell::Cell<Option<(ObjId, u64)>> = const { std::cell::Cell::new(None) };
}

/// The Insert Page Number button for the header / footer field with the caret (placed from the
/// field's current place each frame, so it scrolls with the page). True while the pointer is
/// the button's or its menu's.
pub fn ui(app: &mut App, ui: &mut egui::Ui) -> bool {
    let Some(c) = app.cursor else {
        MENU.with(|m| m.set(None));
        return false;
    };
    let Owner::HeaderFooter { page } = c.owner else {
        MENU.with(|m| m.set(None));
        return false;
    };
    let Some(l) = app.layout.as_ref() else { return false };
    let fields = crate::canvas::hf_fields(l, page);
    let Some((field, _, footer)) = fields.iter().find(|f| f.1 == c.storage).copied() else { return false };
    // Pages hides the button once the header / footer has a page number
    let numbered = app.doc().is_some_and(|d| fields.iter().any(|f| pg_core::edit::page_field_format(d, f.1).is_some()));
    if numbered && MENU.with(|m| m.get()).is_none() {
        return false;
    }
    let band = fields.iter().filter(|f| f.2 == footer).fold(field, |r, f| r.union(f.0));
    let sb = Rect::from_min_max(app.canvas.page_to_screen(l, page, band.min), app.canvas.page_to_screen(l, page, band.max));
    let sf = Rect::from_min_max(app.canvas.page_to_screen(l, page, field.min), app.canvas.page_to_screen(l, page, field.max));
    let size = vec2(168.0, 24.0);
    let br = if footer { Rect::from_center_size(egui::pos2(sf.center().x, sb.min.y - size.y / 2.0 - 6.0), size) } else { Rect::from_center_size(egui::pos2(sf.center().x, sb.max.y + size.y / 2.0 + 6.0), size) };
    if !app.canvas.view_rect().intersects(br) {
        return false;
    }
    let resp = ui.interact(br, ui.scope_id().with(("hf-pagenum", c.storage)), Sense::click());
    let open = MENU.with(|m| m.get()).is_some_and(|m| m.0 == c.storage);
    let hot = resp.hovered() || open;
    let p = ui.painter();
    let (fill, ink) = if hot { (theme::ACCENT(), Color32::WHITE) } else { (Color32::WHITE, theme::ACCENT()) };
    p.rect_filled(br.translate(vec2(0.0, 1.5)), 12.0, Color32::from_black_alpha(30));
    p.rect(br, 12.0, fill, Stroke::new(1.0, theme::ACCENT().gamma_multiply(0.6)), egui::StrokeKind::Inside);
    p.text(br.center() - vec2(7.0, 0.0), egui::Align2::CENTER_CENTER, "Insert Page Number", egui::FontId::proportional(12.0), ink);
    kn_ui::icons::draw(p, Rect::from_center_size(egui::pos2(br.max.x - 14.0, br.center().y), vec2(10.0, 10.0)), kn_ui::icons::Icon::ChevronDown, ink);
    let pass = ui.ctx().cumulative_pass_nr();
    if resp.clicked() {
        MENU.with(|m| m.set(if open { None } else { Some((c.storage, pass)) }));
    }
    let mut over = resp.hovered();
    if let Some((st, opened)) = MENU.with(|m| m.get()).filter(|m| m.0 == c.storage) {
        let at = if footer { br.left_top() - vec2(0.0, 210.0) } else { br.left_bottom() + vec2(0.0, 4.0) };
        let mut closed = false;
        let r = egui::Area::new(ui.scope_id().with(("hf-pagenum-menu", st))).order(egui::Order::Foreground).fixed_pos(at).show(ui.ctx(), |ui| {
            egui::Frame::menu(ui.style()).show(ui, |ui| {
                ui.set_min_width(180.0);
                ui.style_mut().visuals.button_frame = false;
                ui.style_mut().spacing.button_padding = vec2(8.0, 3.0);
                let before = app.version;
                menu_items(app, ui);
                closed = app.version != before;
            })
        });
        over |= ui.input(|i| i.pointer.hover_pos()).is_some_and(|q| r.response.rect.contains(q));
        let just = pass <= opened + 1;
        if closed || (r.response.clicked_elsewhere() && !just && !resp.clicked()) || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            MENU.with(|m| m.set(None));
        }
    }
    over
}
