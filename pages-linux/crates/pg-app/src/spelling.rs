//! Edit > Spelling and Grammar: Check Document Now and the Spelling and Grammar window (the
//! next misspelled word in the sheet's cells, its suggestions, Change / Find Next / Ignore /
//! Learn). kn-ui's spell checker, shared with Keydeck.


use crate::app::{App, Cursor, Owner};

/// The word found: its stored byte range in the body.
#[derive(Clone, Default)]
pub struct Spelling {
    pub at: Option<(usize, usize)>,
    pub word: String,
    pub guesses: Vec<String>,
    pub pick: usize,
    pub done: bool,
}

/// Edit > Spelling and Grammar > Show Spelling and Grammar / Check Document Now.
pub fn open(app: &mut App) {
    app.spelling = Some(Spelling::default());
    find_next(app);
}

/// Find Next: select the next misspelled word of the body after the caret (wrapping round).
pub fn find_next(app: &mut App) {
    let Some(st) = app.wb.as_ref().and_then(|w| w.body_storage()) else { return };
    let text = app.doc().map(|d| d.storage_text(st)).unwrap_or_default();
    let from = app.spelling.as_ref().and_then(|s| s.at).map(|(_, e)| e).or_else(|| app.cursor.filter(|c| c.owner == Owner::Body).map(|c| c.range().1)).unwrap_or(0);
    let found = {
        let bad = kn_ui::spell::misspelled(&text);
        bad.iter().find(|(a, _)| *a >= from).or(bad.first()).copied()
    };
    match found {
        Some((a, b)) => {
            let word = text[a..b].to_string();
            let guesses = kn_ui::spell::suggestions(&word);
            crate::canvas::deselect_objects(app);
            app.cursor = Some(Cursor { storage: st, owner: Owner::Body, anchor: a, caret: b, goal_x: None });
            app.canvas.follow_caret = true;
            app.spelling = Some(Spelling { at: Some((a, b)), word, guesses, pick: 0, done: false });
        }
        None => app.spelling = Some(Spelling { done: true, ..Default::default() }),
    }
}

/// Change: the word becomes the picked suggestion.
fn change(app: &mut App) {
    let Some(sp) = app.spelling.clone() else { return };
    let (Some((a, b)), Some(new)) = (sp.at, sp.guesses.get(sp.pick).cloned()) else { return };
    let Some(st) = app.wb.as_ref().and_then(|w| w.body_storage()) else { return };
    app.cursor = Some(Cursor { storage: st, owner: Owner::Body, anchor: a, caret: b, goal_x: None });
    crate::text::replace_selection(app, &new, "Spelling", false);
    if let Some(s) = app.spelling.as_mut() {
        s.at = Some((a, a + new.len()));
    }
    find_next(app);
}

pub fn window(app: &mut App, ctx: &egui::Context) {
    let Some(mut sp) = app.spelling.clone() else { return };
    let mut open = true;
    let mut act: Option<&str> = None;
    egui::Window::new("Spelling and Grammar").collapsible(false).resizable(false).pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).open(&mut open).show(ctx, |ui| {
        ui.set_width(300.0);
        if sp.done {
            ui.label("No more misspelled words in this document.");
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
