//! Format > Conditional Highlighting…: add a number rule to the selected table cells.

use kn_model::tablecond::{Cmp, LOOKS};

use crate::app::App;

#[derive(Default)]
pub struct CondWindow {
    pub open: bool,
    cmp: usize,
    value: String,
    look: usize,
}

pub fn show(app: &mut App) {
    let w = &mut app.cond_win;
    w.open = true;
    if w.value.is_empty() {
        w.value = "0".into();
        w.cmp = Cmp::ALL.iter().position(|c| *c == Cmp::Greater).unwrap_or(0);
    }
}

pub fn window(app: &mut App, ctx: &egui::Context) {
    if !app.cond_win.open {
        return;
    }
    let range = crate::tableops::range(app);
    let mut open = true;
    let mut add = false;
    egui::Window::new("Conditional Highlighting").pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| {
        let w = &mut app.cond_win;
        match range {
            Some((_, r0, c0, r1, c1)) => ui.label(format!("Rule for {} selected cell{}", (r1 - r0 + 1) * (c1 - c0 + 1), if (r0, c0) == (r1, c1) { "" } else { "s" })),
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
        let ok = range.is_some() && w.value.trim().replace(',', "").parse::<f64>().is_ok();
        if ui.add_enabled(ok, egui::Button::new("Add Rule")).clicked() {
            add = true;
        }
    });
    app.cond_win.open = open;
    if !add {
        return;
    }
    let (Some((id, r0, c0, r1, c1)), Some(slide)) = (range, app.current_slide().map(|s| s.slide)) else { return };
    let w = &app.cond_win;
    let (cmp, look) = (Cmp::ALL[w.cmp], LOOKS[w.look]);
    let Ok(value) = w.value.trim().replace(',', "").parse::<f64>() else { return };
    crate::canvas::commit_cell(app);
    let Some(doc) = app.doc.as_mut() else { return };
    doc.begin(&mut app.hist, "Conditional Highlighting");
    let r = crate::textures::guarded("rule", || doc.add_highlight_rule(&mut app.hist, id, (r0, c0, r1, c1), cmp, value, &look));
    doc.end(&mut app.hist);
    match r {
        Some(Ok(n)) => app.notify(format!("Rule added to {n} cell{}", if n == 1 { "" } else { "s" })),
        Some(Err(e)) => app.notify(e.to_string()),
        None => app.notify("Couldn't add the rule"),
    }
    app.bump(slide);
}
