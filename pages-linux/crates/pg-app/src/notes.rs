//! Comments & Changes: the annotation bar under the toolbar (‹ › through comments and changes,
//! "N Comments & Changes", + Comment, Highlight, Accept / Reject, the Track Changes switch),
//! the comments pane (View > Show Comments Pane) and the word count pill.

use egui::{pos2, vec2, Rect, RichText};
use kn_ui::theme;

use crate::app::{App, Cursor, Owner};

/// Every comment and change of the body, in text order: (stored start, end, comment?, change?).
pub fn annotations(app: &App) -> Vec<(usize, usize, Option<pg_core::notes::TextComment>, Option<pg_core::notes::Change>)> {
    let Some(w) = &app.wb else { return Vec::new() };
    let Some(b) = w.body_storage() else { return Vec::new() };
    let mut v: Vec<_> = w.text_comments(b).into_iter().map(|c| (c.start, c.end, Some(c), None)).collect();
    v.extend(w.changes(b).into_iter().map(|c| (c.start, c.end, None, Some(c))));
    v.sort_by_key(|x| x.0);
    v
}

/// Go to the next / previous comment or change after the caret.
pub fn step(app: &mut App, next: bool) {
    let list = annotations(app);
    if list.is_empty() {
        return;
    }
    let caret = app.cursor.filter(|c| c.owner == Owner::Body).map(|c| c.range().0).unwrap_or(0);
    let pick = if next { list.iter().find(|a| a.0 > caret).or(list.first()) } else { list.iter().rev().find(|a| a.0 < caret).or(list.last()) };
    if let (Some(a), Some(st)) = (pick, app.wb.as_ref().and_then(|w| w.body_storage())) {
        crate::canvas::deselect_objects(app);
        app.cursor = Some(Cursor { storage: st, owner: Owner::Body, anchor: a.0, caret: a.1, goal_x: None });
        app.canvas.follow_caret = true;
    }
}

/// The change at the caret (or the first one in the selection).
fn change_here(app: &App) -> Option<pg_core::notes::Change> {
    let c = app.cursor.filter(|c| c.owner == Owner::Body)?;
    let (a, b) = c.range();
    let w = app.wb.as_ref()?;
    w.changes(c.storage).into_iter().find(|ch| ch.start <= b && ch.end >= a)
}

pub fn accept(app: &mut App, accept: bool) {
    let Some(ch) = change_here(app) else { return };
    let Some(st) = app.wb.as_ref().and_then(|w| w.body_storage()) else { return };
    app.edit_pages(if accept { "Accept Change" } else { "Reject Change" }, |p, h| p.resolve_change(h, st, &ch, accept));
    app.clamp_cursor();
}

pub fn resolve_all(app: &mut App, accept: bool) {
    let Some(st) = app.wb.as_ref().and_then(|w| w.body_storage()) else { return };
    let n = app.edit_pages(if accept { "Accept All Changes" } else { "Reject All Changes" }, |p, h| p.resolve_all(h, st, accept)).unwrap_or(0);
    app.clamp_cursor();
    app.notify(format!("{} {n} {}", if accept { "Accepted" } else { "Rejected" }, if n == 1 { "change" } else { "changes" }));
}

pub fn annotation_bar(app: &mut App, ui: &mut egui::Ui) {
    let list = annotations(app);
    let tracking = app.wb.as_ref().is_some_and(|w| w.tracking());
    ui.horizontal(|ui| {
        if ui.button(if app.comments_pane { "◧" } else { "▢" }).on_hover_text("Show or hide the comments pane").clicked() {
            app.comments_pane = !app.comments_pane;
        }
        if ui.button("‹").clicked() {
            step(app, false);
        }
        if ui.button("›").clicked() {
            step(app, true);
        }
        let nc = list.iter().filter(|a| a.2.is_some()).count();
        let nch = list.iter().filter(|a| a.3.is_some()).count();
        let label = match (nc, nch) {
            (0, 0) => "No Comments or Changes".to_string(),
            (c, 0) => format!("{c} {}", if c == 1 { "Comment" } else { "Comments" }),
            (0, h) => format!("{h} {}", if h == 1 { "Change" } else { "Changes" }),
            (c, h) => format!("{} Comments & Changes", c + h),
        };
        ui.label(RichText::new(label).color(theme::TEXT_DIM()));
        ui.separator();
        let can = app.cursor.is_some_and(|c| c.owner == Owner::Body);
        if ui.add_enabled(can, egui::Button::new("+ Comment")).clicked() {
            crate::menus::run(app, ui.ctx(), "Comment");
        }
        if ui.add_enabled(can && app.cursor.is_some_and(|c| !c.is_empty()), egui::Button::new("Highlight")).clicked() {
            crate::menus::run(app, ui.ctx(), "Highlight");
        }
        if tracking || nch > 0 {
            ui.separator();
            let here = change_here(app).is_some();
            if ui.add_enabled(here, egui::Button::new("Accept")).clicked() {
                accept(app, true);
            }
            if ui.add_enabled(here, egui::Button::new("Reject")).clicked() {
                accept(app, false);
            }
            ui.menu_button("⌄", |ui| {
                if ui.button("Accept All Changes").clicked() {
                    resolve_all(app, true);
                    ui.close();
                }
                if ui.button("Reject All Changes").clicked() {
                    resolve_all(app, false);
                    ui.close();
                }
            });
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let mut on = tracking;
            if theme::toggle(ui, &mut on).changed() {
                app.edit_pages("Track Changes", |p, h| p.set_tracking(h, on));
            }
            ui.label("Track Changes");
        });
    });
}

pub fn comments_pane(app: &mut App, ui: &mut egui::Ui) {
    ui.label(theme::bold("Comments & Changes"));
    ui.separator();
    let list = annotations(app);
    let body = app.wb.as_ref().map(|w| w.body_text()).unwrap_or_default();
    let mut go = None;
    let mut delete = None;
    let mut edit = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        if list.is_empty() {
            ui.label(RichText::new("No comments or changes in this document.").color(theme::TEXT_DIM()));
        }
        for (i, (a, b, c, ch)) in list.iter().enumerate() {
            let (who, color, what) = match (c, ch) {
                (Some(c), _) => (c.author.clone(), c.color, if c.text.is_empty() { "Highlight".to_string() } else { c.text.clone() }),
                (_, Some(ch)) => (ch.author.clone(), ch.color, format!("{} “{}”", if ch.deletion { "Deleted" } else { "Added" }, pg_core::text_plain(&ch.text).chars().take(60).collect::<String>())),
                _ => continue,
            };
            let fr = egui::Frame::new().fill(theme::gray(0x30)).corner_radius(8).inner_margin(8).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), egui::Sense::hover());
                    ui.painter().circle_filled(r.center(), 4.5, crate::objinsp::c32(color));
                    ui.label(theme::bold(if who.is_empty() { "Author".into() } else { who }));
                    if let Some(d) = c.as_ref().and_then(|c| c.date).or(ch.as_ref().and_then(|c| c.date)) {
                        ui.label(RichText::new(pg_core::kn_model::comments::format_date(d)).small().color(theme::TEXT_DIM()));
                    }
                });
                let quote: String = pg_core::text_plain(body.get(*a..*b).unwrap_or("")).chars().take(80).collect();
                if c.is_some() && !quote.is_empty() {
                    ui.label(RichText::new(format!("“{quote}”")).italics().small().color(theme::TEXT_DIM()));
                }
                ui.label(what);
                if let Some(cm) = c {
                    ui.horizontal(|ui| {
                        if ui.small_button("Edit").clicked() {
                            edit = Some((cm.highlight, cm.text.clone()));
                        }
                        if ui.small_button("Delete").clicked() {
                            delete = Some(cm.highlight);
                        }
                    });
                }
            });
            // a click on the card (not its buttons) goes to the text; a click sense laid over
            // the card would take the buttons' clicks
            if delete.is_none() && edit.is_none() && ui.rect_contains_pointer(fr.response.rect) && ui.input(|i| i.pointer.primary_clicked()) {
                go = Some(i);
            }
            ui.add_space(6.0);
        }
    });
    if let Some(i) = go {
        if let (Some(a), Some(st)) = (list.get(i), app.wb.as_ref().and_then(|w| w.body_storage())) {
            app.cursor = Some(Cursor { storage: st, owner: Owner::Body, anchor: a.0, caret: a.1, goal_x: None });
            app.canvas.follow_caret = true;
        }
    }
    if let Some(h) = delete {
        if let Some(st) = app.wb.as_ref().and_then(|w| w.body_storage()) {
            app.edit_pages("Delete Comment", |p, hi| p.delete_text_comment(hi, st, h));
        }
    }
    if let Some((h, t)) = edit {
        app.comment_edit = Some((Some(h), t));
    }
}

/// The word count pill at the bottom of the canvas (click to switch what it counts).
pub fn word_count_pill(app: &mut App, ui: &egui::Ui, rect: Rect) {
    let text = app.wb.as_ref().map(|w| pg_core::text_plain(&w.body_text())).unwrap_or_default();
    let (sel_text, has_sel) = match app.cursor {
        Some(c) if !c.is_empty() => (crate::text::selected_stored(app).map(|s| pg_core::text_plain(&s)).unwrap_or_default(), true),
        _ => (String::new(), false),
    };
    let t = if has_sel { &sel_text } else { &text };
    let n = match app.word_count_mode {
        1 => t.chars().filter(|c| *c != '\n').count(),
        2 => t.chars().filter(|c| !c.is_whitespace()).count(),
        3 => t.split('\n').filter(|l| !l.trim().is_empty()).count(),
        4 => app.page_count(),
        _ => t.split_whitespace().filter(|w| w.chars().any(|c| c.is_alphanumeric())).count(),
    };
    let what = ["words", "characters", "characters without spaces", "paragraphs", "pages"][app.word_count_mode as usize % 5];
    let label = if has_sel && app.word_count_mode != 4 { format!("{} of {} {what}", group(n), group(count_all(&text, app.word_count_mode))) } else { format!("{} {what}", group(n)) };
    let g = ui.painter().layout_no_wrap(format!("{label}  ⇕"), egui::FontId::proportional(12.5), theme::TEXT());
    let r = Rect::from_center_size(pos2(rect.center().x, rect.max.y - 26.0), vec2(g.size().x + 28.0, 27.0));
    let resp = ui.interact(r, ui.scope_id().with("word-count"), egui::Sense::click());
    ui.painter().rect_filled(r, 13.5, theme::gray(if resp.hovered() { 0x3a } else { 0x2c }).gamma_multiply(0.96));
    ui.painter().galley(r.center() - g.size() / 2.0, g, theme::TEXT());
    if resp.clicked() {
        app.word_count_mode = (app.word_count_mode + 1) % 5;
    }
}

fn count_all(t: &str, mode: u8) -> usize {
    match mode {
        1 => t.chars().filter(|c| *c != '\n').count(),
        2 => t.chars().filter(|c| !c.is_whitespace()).count(),
        3 => t.split('\n').filter(|l| !l.trim().is_empty()).count(),
        _ => t.split_whitespace().filter(|w| w.chars().any(|c| c.is_alphanumeric())).count(),
    }
}

/// 2,028
fn group(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}
