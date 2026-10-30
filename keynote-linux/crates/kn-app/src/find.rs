//! Edit > Find > Find & Replace: searches slide text and presenter notes in slide order.

use egui::RichText;
use kn_format::ObjId;
use kn_model::DrawableKind;

use crate::app::App;

#[derive(Default)]
pub struct FindState {
    pub open: bool,
    pub query: String,
    pub replace: String,
    pub match_case: bool,
    pub whole_words: bool,
    /// Matches: (slide index, storage, byte start, byte end).
    matches: Vec<(usize, ObjId, usize, usize)>,
    current: usize,
    searched_for: String,
    focus: bool,
}

impl FindState {
    pub fn show(&mut self) {
        self.open = true;
        self.focus = true;
    }
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn find_all(text: &str, q: &str, case: bool, words: bool) -> Vec<(usize, usize)> {
    if q.is_empty() {
        return Vec::new();
    }
    let (hay, needle) = if case { (text.to_string(), q.to_string()) } else { (text.to_lowercase(), q.to_lowercase()) };
    // lowercase can change byte lengths; only trust positions when lengths agree
    if hay.len() != text.len() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = hay[from..].find(&needle) {
        let s = from + i;
        let e = s + needle.len();
        let ok = !words
            || (text[..s].chars().last().is_none_or(|c| !is_word_char(c)) && text[e..].chars().next().is_none_or(|c| !is_word_char(c)));
        if ok {
            out.push((s, e));
        }
        from = e.max(s + 1);
        while from < hay.len() && !hay.is_char_boundary(from) {
            from += 1;
        }
    }
    out
}

fn search(app: &mut App) {
    let (q, case, words) = (app.find.query.clone(), app.find.match_case, app.find.whole_words);
    let mut matches = Vec::new();
    for i in 0..app.slides.len() {
        let Some(sc) = app.scene(i) else { continue };
        let mut storages: Vec<ObjId> = sc
            .drawables
            .iter()
            .filter(|d| !d.from_template)
            .filter_map(|d| match &d.kind {
                DrawableKind::Shape(s) => s.text.as_ref().map(|t| t.storage),
                _ => None,
            })
            .collect();
        if let Some(n) = app.doc.as_ref().and_then(|d| d.notes_storage(sc.slide)) {
            storages.push(n);
        }
        for st in storages {
            let text = app.doc.as_ref().map(|d| d.storage_text(st)).unwrap_or_default();
            for (s, e) in find_all(&text, &q, case, words) {
                matches.push((i, st, s, e));
            }
        }
    }
    app.find.matches = matches;
    app.find.current = 0;
    app.find.searched_for = format!("{q}|{case}|{words}");
}

/// Find Next / Find Previous from the menu or Ctrl+G (opens the panel if nothing searched yet).
pub fn step(app: &mut App, forward: bool) {
    if app.find.query.is_empty() {
        app.find.show();
        return;
    }
    let key = format!("{}|{}|{}", app.find.query, app.find.match_case, app.find.whole_words);
    if key != app.find.searched_for {
        search(app);
    } else if !app.find.matches.is_empty() {
        let n = app.find.matches.len();
        app.find.current = if forward { (app.find.current + 1) % n } else { (app.find.current + n - 1) % n };
    }
    if app.find.matches.is_empty() {
        app.notify("Not found");
    }
    reveal(app);
}

fn reveal(app: &mut App) {
    if let Some(&(i, st, _, _)) = app.find.matches.get(app.find.current) {
        if app.cur != i {
            app.go_to(i);
        }
        // select the object holding the match
        if let Some(sc) = app.scene(i) {
            if let Some(d) = sc.drawables.iter().find(|d| matches!(&d.kind, DrawableKind::Shape(s) if s.text.as_ref().is_some_and(|t| t.storage == st))) {
                app.sel = vec![d.id];
            }
        }
    }
}

pub fn ui(app: &mut App, ctx: &egui::Context) {
    if !app.find.open {
        return;
    }
    let mut open = true;
    let mut action: Option<&str> = None;
    egui::Window::new("Find & Replace").pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).collapsible(false).resizable(false).default_width(340.0).open(&mut open).show(ctx, |ui| {
        let f = &mut app.find;
        let r = ui.add(egui::TextEdit::singleline(&mut f.query).hint_text("Find").desired_width(f32::INFINITY));
        if f.focus {
            r.request_focus();
            f.focus = false;
        }
        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            action = Some("next");
        }
        ui.add(egui::TextEdit::singleline(&mut f.replace).hint_text("Replace").desired_width(f32::INFINITY));
        ui.horizontal(|ui| {
            ui.checkbox(&mut f.match_case, "Match Case");
            ui.checkbox(&mut f.whole_words, "Whole Words");
        });
        let key = format!("{}|{}|{}", f.query, f.match_case, f.whole_words);
        let fresh = key == f.searched_for;
        ui.horizontal(|ui| {
            if fresh && !f.query.is_empty() {
                let n = f.matches.len();
                ui.label(RichText::new(if n == 0 { "Not found".to_string() } else { format!("{} of {n}", f.current + 1) }).color(crate::theme::TEXT_DIM()));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("▶").on_hover_text("Next").clicked() {
                    action = Some("next");
                }
                if ui.button("◀").on_hover_text("Previous").clicked() {
                    action = Some("prev");
                }
            });
        });
        ui.horizontal(|ui| {
            if ui.button("Replace").clicked() {
                action = Some("replace");
            }
            if ui.button("Replace All").clicked() {
                action = Some("all");
            }
        });
    });
    app.find.open = open;
    let Some(action) = action else { return };
    crate::textedit::commit(app);
    let key = format!("{}|{}|{}", app.find.query, app.find.match_case, app.find.whole_words);
    if key != app.find.searched_for {
        search(app);
        if action == "next" || action == "prev" {
            reveal(app);
            return;
        }
    }
    let n = app.find.matches.len();
    match action {
        "next" if n > 0 => {
            app.find.current = (app.find.current + 1) % n;
            reveal(app);
        }
        "prev" if n > 0 => {
            app.find.current = (app.find.current + n - 1) % n;
            reveal(app);
        }
        "replace" if n > 0 => {
            let (i, st, s, e) = app.find.matches[app.find.current];
            let with = app.find.replace.clone();
            if let Some(doc) = app.doc.as_mut() {
                doc.begin(&mut app.hist, "Replace");
                doc.replace_text(&mut app.hist, st, s, e, &with);
                doc.end(&mut app.hist);
            }
            if let Some(sl) = app.slides.get(i).map(|s| s.slide) {
                app.bump(sl);
            }
            app.notes_buf = None;
            search(app);
            reveal(app);
        }
        "all" if n > 0 => {
            let with = app.find.replace.clone();
            let mut ms = app.find.matches.clone();
            // replace back to front so earlier offsets stay valid
            ms.sort_by(|a, b| (a.1, b.2).cmp(&(b.1, a.2)));
            if let Some(doc) = app.doc.as_mut() {
                doc.begin(&mut app.hist, "Replace All");
                for (_, st, s, e) in &ms {
                    doc.replace_text(&mut app.hist, *st, *s, *e, &with);
                }
                doc.end(&mut app.hist);
            }
            app.notes_buf = None;
            app.bump_all();
            app.notify(format!("Replaced {n} occurrence{}", if n == 1 { "" } else { "s" }));
            search(app);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::find_all;

    #[test]
    fn finds_case_and_words() {
        assert_eq!(find_all("Rest is rest; restful", "rest", false, false).len(), 3);
        assert_eq!(find_all("Rest is rest; restful", "rest", true, false).len(), 2);
        assert_eq!(find_all("Rest is rest; restful", "rest", false, true).len(), 2);
    }
}
