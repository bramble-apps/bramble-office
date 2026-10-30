//! Format > Conditional Highlighting… (and the Cell pane's button): a number rule for the
//! selected table cells, or clear their rules. Keydeck's window, on kn-model's tablecond.

use pg_core::kn_model::tablecond::{Cmp, LOOKS};

use crate::app::App;

#[derive(Default)]
struct State {
    open: bool,
    cmp: usize,
    value: String,
    look: usize,
}

thread_local! {
    static STATE: std::cell::RefCell<State> = std::cell::RefCell::new(State::default());
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| f(&mut s.borrow_mut()))
}

pub fn show() {
    with(|w| {
        w.open = true;
        if w.value.is_empty() {
            w.value = "0".into();
            w.cmp = Cmp::ALL.iter().position(|c| *c == Cmp::Greater).unwrap_or(0);
        }
    });
}

pub fn ui(app: &mut App, ctx: &egui::Context) {
    if !with(|w| w.open) {
        return;
    }
    let range = app.sel.map(|s| {
        let (r0, c0, r1, c1) = s.block();
        (s.table, r0, c0, r1, c1)
    });
    let can = range.is_some_and(|(t, ..)| app.doc().is_some_and(|d| d.can_highlight(t)));
    let has_rules = range.is_some_and(|(t, r0, c0, r1, c1)| app.doc().is_some_and(|d| (r0..=r1).any(|r| (c0..=c1).any(|c| !d.highlight_rules(t, r, c).is_empty()))));
    let mut open = true;
    let (mut add, mut clear) = (false, false);
    egui::Window::new("Conditional Highlighting").pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| {
        with(|w| {
            match range {
                Some((_, r0, c0, r1, c1)) if can => ui.label(format!("Rule for {} selected cell{}", (r1 - r0 + 1) * (c1 - c0 + 1), if (r0, c0) == (r1, c1) { "" } else { "s" })),
                Some(_) => ui.label(egui::RichText::new("This table can't take rules.").italics()),
                None => ui.label(egui::RichText::new("Select table cells first.").italics()),
            };
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("Number");
                egui::ComboBox::from_id_salt("cond-cmp").selected_text(Cmp::ALL[w.cmp].name()).show_ui(ui, |ui| {
                    for (i, c) in Cmp::ALL.iter().enumerate() {
                        ui.selectable_value(&mut w.cmp, i, c.name());
                    }
                });
                ui.add(egui::TextEdit::singleline(&mut w.value).desired_width(70.0));
            });
            ui.horizontal(|ui| {
                ui.label("Style");
                egui::ComboBox::from_id_salt("cond-look").selected_text(LOOKS[w.look].name).show_ui(ui, |ui| {
                    for (i, l) in LOOKS.iter().enumerate() {
                        ui.selectable_value(&mut w.look, i, l.name);
                    }
                });
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let ok = can && w.value.trim().replace(',', "").parse::<f64>().is_ok();
                add = ui.add_enabled(ok, egui::Button::new("Add Rule")).clicked();
                clear = ui.add_enabled(has_rules, egui::Button::new("Clear All Rules")).clicked();
            });
        });
    });
    with(|w| w.open = open);
    let Some((t, r0, c0, r1, c1)) = range else { return };
    if clear {
        crate::tables::commit(app);
        if let Some(Err(e)) = app.edit("Clear Rules", |d, h| d.clear_highlight_rules(h, t, (r0, c0, r1, c1))) {
            app.notify(e.to_string());
        }
        return;
    }
    if !add {
        return;
    }
    let (cmp, look, value) = with(|w| (Cmp::ALL[w.cmp], LOOKS[w.look], w.value.trim().replace(',', "").parse::<f64>()));
    let Ok(value) = value else { return };
    crate::tables::commit(app);
    match app.edit("Conditional Highlighting", |d, h| d.add_highlight_rule(h, t, (r0, c0, r1, c1), cmp, value, &look)) {
        Some(Ok(n)) => app.notify(format!("Rule added to {n} cell{}", if n == 1 { "" } else { "s" })),
        Some(Err(e)) => app.notify(e.to_string()),
        None => {}
    }
}
