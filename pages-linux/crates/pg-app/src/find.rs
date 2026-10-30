//! Edit > Find (Ctrl+F) / Find & Replace: the bar under the toolbar, matches in the body
//! highlighted, the current one selected; Match Case and Whole Words options; Replace,
//! Replace All (one undo step).

use egui::RichText;
use kn_ui::theme;

use crate::app::{App, Cursor, Owner};

/// Stored byte ranges of `q` in the body.
pub fn search(app: &App, q: &str) -> Vec<(usize, usize)> {
    if q.is_empty() {
        return Vec::new();
    }
    let Some(w) = &app.wb else { return Vec::new() };
    let text = w.body_text();
    let (hay, needle) = if app.find_case { (text.clone(), q.to_string()) } else { (text.to_lowercase(), q.to_lowercase()) };
    // lower-casing can change byte lengths: then only exact matches are safe
    if hay.len() != text.len() {
        return text.match_indices(q).map(|(i, m)| (i, i + m.len())).collect();
    }
    let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric());
    hay.match_indices(&needle)
        .map(|(i, m)| (i, i + m.len()))
        .filter(|(a, b)| !app.find_words || (!word(text[..*a].chars().next_back()) && !word(text[*b..].chars().next())))
        .collect()
}

/// Show match `i` (select it, scroll to it).
pub fn show_match(app: &mut App, i: usize) {
    let Some((q, matches, _)) = app.find.clone() else { return };
    let Some(&(a, b)) = matches.get(i) else { return };
    app.find = Some((q, matches, i));
    if let Some(st) = app.wb.as_ref().and_then(|w| w.body_storage()) {
        crate::canvas::deselect_objects(app);
        app.cursor = Some(Cursor { storage: st, owner: Owner::Body, anchor: a, caret: b, goal_x: None });
        app.canvas.follow_caret = true;
    }
}

/// Find Next / Previous.
pub fn step(app: &mut App, next: bool) {
    let Some((q, _, cur)) = app.find.clone() else { return };
    let matches = search(app, &q);
    if matches.is_empty() {
        app.find = Some((q, matches, 0));
        return;
    }
    // from the caret
    let caret = app.cursor.filter(|c| c.owner == Owner::Body).map(|c| if next { c.range().1 } else { c.range().0 });
    let i = match caret {
        Some(c) if next => matches.iter().position(|m| m.0 >= c).unwrap_or(0),
        Some(c) => matches.iter().rposition(|m| m.1 <= c).unwrap_or(matches.len() - 1),
        None => (cur + if next { 1 } else { matches.len() - 1 }) % matches.len(),
    };
    app.find = Some((q, matches, i));
    show_match(app, i);
}

pub fn replace(app: &mut App, all: bool) {
    let (Some((q, _, cur)), Some(with)) = (app.find.clone(), app.replace.clone()) else { return };
    let matches = search(app, &q);
    let Some(st) = app.wb.as_ref().and_then(|w| w.body_storage()) else { return };
    if matches.is_empty() {
        return;
    }
    let list: Vec<(usize, usize)> = if all { matches.clone() } else { vec![matches[cur.min(matches.len() - 1)]] };
    let n = list.len();
    app.edit(if all { "Replace All" } else { "Replace" }, |d, h| {
        for (a, b) in list.iter().rev() {
            pg_core::edit::replace(d, h, st, *a, *b, &with);
        }
    });
    let m = search(app, &q);
    app.find = Some((q, m, cur));
    if all {
        app.notify(format!("Replaced {n} {}", if n == 1 { "match" } else { "matches" }));
    } else {
        step(app, true);
    }
}

pub fn find_bar(app: &mut App, ui: &mut egui::Ui) {
    let Some((mut q, _, cur)) = app.find.clone() else { return };
    let mut close = false;
    ui.horizontal(|ui| {
        let r = ui.add(egui::TextEdit::singleline(&mut q).hint_text("Find").desired_width(220.0));
        if ui.memory(|m| m.focused().is_none()) && app.cursor.is_none_or(|c| c.is_empty()) {
            r.request_focus();
        }
        if r.changed() {
            let m = search(app, &q);
            app.find = Some((q.clone(), m, 0));
            if app.find.as_ref().is_some_and(|f| !f.1.is_empty()) {
                show_match(app, 0);
            }
        }
        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            step(app, !ui.input(|i| i.modifiers.shift));
            r.request_focus();
        }
        let n = app.find.as_ref().map(|f| f.1.len()).unwrap_or(0);
        ui.label(RichText::new(if q.is_empty() { String::new() } else if n == 0 { "Not found".into() } else { format!("{} of {n}", cur.min(n - 1) + 1) }).color(theme::TEXT_DIM()));
        if ui.button("‹").on_hover_text("Previous (Shift+Ctrl+G)").clicked() {
            step(app, false);
        }
        if ui.button("›").on_hover_text("Next (Ctrl+G)").clicked() {
            step(app, true);
        }
        ui.separator();
        if theme::checkbox(ui, &mut app.find_case, "Match Case").changed() || theme::checkbox(ui, &mut app.find_words, "Whole Words").changed() {
            let m = search(app, &q);
            app.find = Some((q.clone(), m, 0));
        }
        ui.separator();
        match app.replace.as_mut() {
            Some(with) => {
                ui.add(egui::TextEdit::singleline(with).hint_text("Replace").desired_width(180.0));
                if ui.button("Replace").clicked() {
                    replace(app, false);
                }
                if ui.button("Replace All").clicked() {
                    replace(app, true);
                }
            }
            None => {
                if ui.button("Find & Replace").clicked() {
                    app.replace = Some(String::new());
                }
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Done").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                close = true;
            }
        });
    });
    if close {
        app.find = None;
        app.replace = None;
    }
}
