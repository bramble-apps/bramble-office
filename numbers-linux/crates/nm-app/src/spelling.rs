//! Edit > Spelling and Grammar: Check Document Now and the Spelling and Grammar window (the
//! next misspelled word in the sheet's cells, its suggestions, Change / Find Next / Ignore /
//! Learn). kn-ui's spell checker, shared with Keydeck.

use nm_core::kn_format::ObjId;
use nm_core::kn_model::DrawableKind;

use crate::app::{App, Sel};

/// The word found: its table, shown row, column, and byte range in the cell's text.
#[derive(Clone, Default)]
pub struct Spelling {
    pub at: Option<(ObjId, u32, u32, usize, usize)>,
    pub word: String,
    pub guesses: Vec<String>,
    pub pick: usize,
    pub done: bool,
}

/// The sheet's text cells in reading order, from just after the current one.
fn cells(app: &App) -> Vec<(ObjId, u32, u32, String)> {
    let mut all = Vec::new();
    for d in &app.drawables {
        let DrawableKind::Table(t) = &d.kind else { continue };
        for r in 0..t.rows {
            for c in 0..t.cols {
                let text = t.cells.get((r * t.cols + c) as usize).cloned().unwrap_or_default();
                if text.chars().any(|ch| ch.is_alphabetic()) && app.doc().and_then(|doc| doc.formula_text(d.id, app.mrow(d.id, r).unwrap_or(r), c)).is_none() {
                    all.push((d.id, r, c, text));
                }
            }
        }
    }
    all
}

/// Check Document Now / Find Next: select the next cell with a misspelled word.
pub fn find_next(app: &mut App) {
    let all = cells(app);
    let from = app.spelling.as_ref().and_then(|s| s.at).map(|(t, r, c, _, e)| (t, r, c, e));
    let start = from.and_then(|(t, r, c, _)| all.iter().position(|x| (x.0, x.1, x.2) == (t, r, c))).unwrap_or(0);
    let n = all.len();
    for k in 0..=n {
        let i = (start + k) % n.max(1);
        let Some((t, r, c, text)) = all.get(i) else { break };
        // in the cell we were in, only words after the last one
        let after = if k == 0 { from.map(|f| f.3).unwrap_or(0) } else { 0 };
        if let Some(&(a, b)) = kn_ui::spell::misspelled(text).iter().find(|(a, _)| *a >= after) {
            let word = text[a..b].to_string();
            let guesses = kn_ui::spell::suggestions(&word);
            app.sel = Some(Sel::cell(*t, *r, *c));
            app.reveal_cell(*t, *r, *c);
            app.spelling = Some(Spelling { at: Some((*t, *r, *c, a, b)), word, guesses, pick: 0, done: false });
            return;
        }
    }
    app.spelling = Some(Spelling { done: true, ..Default::default() });
}

/// Change: the word in its cell becomes the picked suggestion.
fn change(app: &mut App) {
    let Some(sp) = app.spelling.clone() else { return };
    let (Some((t, r, c, a, b)), Some(new)) = (sp.at, sp.guesses.get(sp.pick).cloned()) else { return };
    let Some(text) = app.table(t).and_then(|(_, td)| td.cells.get((r * td.cols + c) as usize).cloned()) else { return };
    if b > text.len() || &text[a..b] != sp.word {
        return;
    }
    let fixed = format!("{}{new}{}", &text[..a], &text[b..]);
    let mr = app.mrow(t, r).unwrap_or(r);
    app.edit("Spelling", |d, h| d.set_cell(h, t, mr, c, &nm_core::kn_model::tablecells::CellInput::Text(fixed)));
    // go on from after the changed word
    if let Some(s) = app.spelling.as_mut() {
        s.at = Some((t, r, c, a, a + new.len()));
    }
    find_next(app);
}

pub fn window(app: &mut App, ctx: &egui::Context) {
    let Some(mut sp) = app.spelling.clone() else { return };
    let mut open = true;
    let mut act: Option<&str> = None;
    egui::Window::new("Spelling and Grammar").collapsible(false).resizable(false).default_pos(ctx.content_rect().center()).open(&mut open).show(ctx, |ui| {
        ui.set_width(300.0);
        if sp.done {
            ui.label("No more misspelled words in this sheet.");
            return;
        }
        if kn_ui::spell::dicts().is_some_and(|d| d.is_empty()) {
            ui.label("No spelling dictionaries are installed (hunspell-en-us, hunspell-fr).");
            return;
        }
        ui.label(kn_ui::theme::bold(&sp.word).color(egui::Color32::from_rgb(0xe0, 0x1b, 0x24)));
        ui.add_space(4.0);
        egui::Frame::new().fill(ui.visuals().extreme_bg_color).corner_radius(6).inner_margin(4).show(ui, |ui| {
            ui.set_min_height(110.0);
            ui.set_width(292.0);
            if sp.guesses.is_empty() {
                ui.label(egui::RichText::new("No guesses").weak());
            }
            for (i, g) in sp.guesses.iter().enumerate() {
                if ui.selectable_label(sp.pick == i, g).clicked() {
                    sp.pick = i;
                }
            }
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.add_enabled(!sp.guesses.is_empty(), egui::Button::new("Change")).clicked() {
                act = Some("change");
            }
            if ui.button("Find Next").clicked() {
                act = Some("next");
            }
            if ui.button("Ignore").clicked() {
                act = Some("ignore");
            }
            if ui.button("Learn").clicked() {
                act = Some("learn");
            }
        });
    });
    app.spelling = if open { Some(sp.clone()) } else { None };
    match act {
        Some("change") => change(app),
        Some("next") => find_next(app),
        Some("ignore") => {
            kn_ui::spell::ignore(&sp.word);
            find_next(app);
        }
        Some("learn") => {
            kn_ui::spell::learn(&sp.word);
            find_next(app);
        }
        _ => {}
    }
}
