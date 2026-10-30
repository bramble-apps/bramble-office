//! Edit > Spelling and Grammar: the Spelling window (Show Spelling and Grammar), Check Document
//! Now (select the next misspelled word), and Edit > Substitutions > Show Substitutions.

use kn_format::ObjId;
use kn_model::DrawableKind;

use crate::app::App;

#[derive(Default)]
pub struct SpellWindow {
    pub open: bool,
    /// The word found last and its suggestions.
    word: Option<String>,
    guesses: Vec<String>,
    pick: usize,
    change_to: String,
    pub substitutions_open: bool,
    message: Option<String>,
}

/// Text objects of slide `i` in drawing order: (object, text storage).
fn texts(app: &mut App, i: usize) -> Vec<(ObjId, ObjId)> {
    let Some(sc) = app.scene(i) else { return Vec::new() };
    sc.drawables
        .iter()
        .filter(|d| !d.from_template)
        .filter_map(|d| match &d.kind {
            DrawableKind::Shape(s) => s.text.as_ref().map(|t| (d.id, t.storage)),
            _ => None,
        })
        .collect()
}

/// Select the next misspelled (or, with grammar on, questionable) word after the caret, going
/// through the slides in order and wrapping around once. Returns the word.
pub fn find_next(app: &mut App) -> Option<String> {
    let n = app.slides.len();
    if n == 0 {
        return None;
    }
    // where to start: the caret of the text being edited, else the current slide's start
    let (start_slide, start_obj, start_at) = match app.canvas.editing.as_ref() {
        Some(e) => (app.cur, Some(e.id), e.cursor.max(e.anchor)),
        None => (app.cur, None, 0),
    };
    crate::textedit::commit(app);
    for k in 0..=n {
        let i = (start_slide + k) % n;
        let objs = texts(app, i);
        let mut skipping = k == 0 && start_obj.is_some();
        for (id, storage) in objs {
            if skipping {
                if Some(id) == start_obj {
                    skipping = false;
                } else {
                    continue;
                }
            }
            let text = app.doc.as_ref().map(|d| d.storage_text(storage)).unwrap_or_default();
            let mut marks = crate::spell::misspelled(&text);
            if app.settings.grammar {
                marks.extend(crate::spell::grammar(&text));
            }
            marks.sort();
            let from = if k == 0 && Some(id) == start_obj { start_at } else { 0 };
            // after wrapping all the way round, stop at the start point
            let until = if k == n && Some(id) == start_obj { start_at } else { usize::MAX };
            if let Some(&(s, e)) = marks.iter().find(|(s, _)| *s >= from && *s < until) {
                app.go_to(i);
                let Some(scene) = app.scene(i) else { return None };
                if crate::textedit::begin(app, &scene, id, None) {
                    app.sel = vec![id];
                    if let Some(ed) = app.canvas.editing.as_mut() {
                        ed.anchor = s;
                        ed.cursor = e;
                    }
                    return Some(text[s..e].to_string());
                }
            }
        }
    }
    None
}

/// Edit > Spelling and Grammar > Check Document Now.
pub fn check_now(app: &mut App) {
    if find_next(app).is_none() {
        app.notify("No spelling or grammar issues found");
    }
}

fn found(app: &mut App, word: Option<String>) {
    let w = &mut app.spell_win;
    w.guesses = word.as_deref().map(crate::spell::suggestions).unwrap_or_default();
    w.change_to = w.guesses.first().cloned().or_else(|| word.clone()).unwrap_or_default();
    w.pick = 0;
    w.message = word.is_none().then(|| "No spelling or grammar issues found.".to_string());
    w.word = word;
}

pub fn show(app: &mut App) {
    app.spell_win.open = true;
    let w = find_next(app);
    found(app, w);
}

pub fn window(app: &mut App, ctx: &egui::Context) {
    if app.spell_win.substitutions_open {
        let mut open = true;
        egui::Window::new("Substitutions").pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| {
            let s = &mut app.settings;
            let before = s.clone();
            ui.checkbox(&mut s.smart_quotes, "Smart Quotes");
            ui.checkbox(&mut s.smart_dashes, "Smart Dashes");
            ui.checkbox(&mut s.text_replacement, "Text Replacement");
            ui.checkbox(&mut s.smart_links, "Smart Links");
            ui.checkbox(&mut s.smart_phone_links, "Smart Phone Number Links");
            ui.separator();
            ui.label(egui::RichText::new("Text replacements").strong());
            egui::Grid::new("replacements").num_columns(2).spacing([16.0, 2.0]).show(ui, |ui| {
                for (from, to) in crate::textedit::REPLACEMENTS {
                    ui.label(*from);
                    ui.label(*to);
                    ui.end_row();
                }
            });
            if *s != before {
                s.save();
            }
        });
        app.spell_win.substitutions_open = open;
    }
    if !app.spell_win.open {
        return;
    }
    let mut open = true;
    let mut act: Option<&str> = None;
    egui::Window::new("Spelling and Grammar").pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).collapsible(false).resizable(false).default_width(340.0).open(&mut open).show(ctx, |ui| {
        let w = &mut app.spell_win;
        match &w.word {
            Some(word) => {
                ui.label(egui::RichText::new(format!("\u{201c}{word}\u{201d}")).strong());
            }
            None => {
                ui.label(w.message.clone().unwrap_or_default());
            }
        }
        ui.add(egui::TextEdit::singleline(&mut w.change_to).desired_width(f32::INFINITY));
        egui::ScrollArea::vertical().max_height(140.0).show(ui, |ui| {
            if w.guesses.is_empty() && w.word.is_some() {
                ui.label(egui::RichText::new("No Guesses Found").italics());
            }
            for (i, g) in w.guesses.clone().into_iter().enumerate() {
                if ui.add(egui::Button::new(&g).selected(i == w.pick).frame_when_inactive(false)).clicked() {
                    w.pick = i;
                    w.change_to = g;
                }
            }
        });
        ui.horizontal(|ui| {
            let has = w.word.is_some();
            if ui.add_enabled(has, egui::Button::new("Change")).clicked() {
                act = Some("change");
            }
            if ui.button("Find Next").clicked() {
                act = Some("next");
            }
            if ui.add_enabled(has, egui::Button::new("Ignore")).clicked() {
                act = Some("ignore");
            }
            if ui.add_enabled(has, egui::Button::new("Learn")).clicked() {
                act = Some("learn");
            }
        });
        let s = &mut app.settings;
        let before = s.clone();
        ui.checkbox(&mut s.grammar, "Check grammar");
        if *s != before {
            s.save();
        }
    });
    app.spell_win.open = open;
    let word = app.spell_win.word.clone();
    match act {
        Some("change") => {
            let to = app.spell_win.change_to.clone();
            if !crate::textedit::insert_at_caret(app, &to) {
                app.notify("Select the word in its text box first");
            }
            let w = find_next(app);
            found(app, w);
        }
        Some("ignore") | Some("learn") => {
            if let Some(w) = &word {
                if act == Some("learn") {
                    crate::spell::learn(w);
                } else {
                    crate::spell::ignore(w);
                }
            }
            let w = find_next(app);
            found(app, w);
        }
        Some(_) => {
            let w = find_next(app);
            found(app, w);
        }
        None => {}
    }
}
