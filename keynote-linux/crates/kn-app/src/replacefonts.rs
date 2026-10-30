//! Format > Font > Replace Fonts…: every font the presentation uses, flagged when it is not
//! installed here, with a replacement to pick (applied to all styles, like Keynote).

use egui::RichText;

use crate::app::App;
use crate::theme;

pub struct ReplaceFonts {
    /// (font name in the file, installed, chosen replacement family)
    rows: Vec<(String, bool, Option<String>)>,
}

pub fn open(app: &mut App) {
    let Some(doc) = app.doc.as_ref() else { return };
    let rows = doc.fonts_used().into_iter().map(|(f, _)| {
        let installed = app.renderer.fonts.is_installed(&f);
        (f, installed, None)
    });
    app.replace_fonts = Some(ReplaceFonts { rows: rows.collect() });
}

pub fn ui(app: &mut App, ctx: &egui::Context) {
    let Some(mut st) = app.replace_fonts.take() else { return };
    let families: Vec<String> = app.renderer.fonts.families().to_vec();
    let mut open = true;
    let mut apply = false;
    egui::Window::new("Replace Fonts").pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).collapsible(false).resizable(true).default_width(520.0).open(&mut open).show(ctx, |ui| {
        ui.label(RichText::new("Fonts marked ⚠ are not installed on this computer and are shown with a substitute.").color(theme::TEXT_DIM()));
        ui.add_space(6.0);
        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
            egui::Grid::new("fonts").num_columns(3).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label(RichText::new("Font").strong());
                ui.label("");
                ui.label(RichText::new("Replace with").strong());
                ui.end_row();
                for (i, (name, installed, to)) in st.rows.iter_mut().enumerate() {
                    ui.label(if *installed { RichText::new(name.as_str()) } else { RichText::new(format!("⚠ {name}")).color(egui::Color32::from_rgb(230, 160, 60)) });
                    ui.label("→");
                    egui::ComboBox::from_id_salt(("replace", i)).width(220.0).selected_text(to.clone().unwrap_or_else(|| "Don't replace".into())).show_ui(ui, |ui| {
                        ui.selectable_value(to, None, "Don't replace");
                        for f in &families {
                            ui.selectable_value(to, Some(f.clone()), f);
                        }
                    });
                    ui.end_row();
                }
            });
        });
        ui.add_space(8.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let any = st.rows.iter().any(|r| r.2.is_some());
            if ui.add_enabled(any, egui::Button::new(RichText::new("Replace").strong())).clicked() {
                apply = true;
            }
        });
    });
    if apply {
        let changes: Vec<(String, String)> = st
            .rows
            .iter()
            .filter_map(|(from, _, to)| to.as_ref().map(|fam| (from.clone(), app.renderer.fonts.ps_name_for_family(fam).unwrap_or_else(|| fam.clone()))))
            .collect();
        let mut n = 0;
        if let Some(doc) = app.doc.as_mut() {
            doc.begin(&mut app.hist, "Replace Fonts");
            for (from, to) in &changes {
                n += doc.replace_font(&mut app.hist, from, to);
            }
            doc.end(&mut app.hist);
        }
        app.font_names = None;
        app.renderer.clear_caches();
        app.bump_all();
        app.notify(format!("Replaced fonts in {n} style{}", if n == 1 { "" } else { "s" }));
        return;
    }
    if open {
        app.replace_fonts = Some(st);
    }
}
