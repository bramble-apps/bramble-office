//! Format > Font > Show Fonts (the Mac's font panel: family, typeface, size, underline /
//! strikethrough and colour, applied to the selection as you pick) and Replace Fonts… (every
//! font the spreadsheet uses, flagged when it isn't installed, with a replacement to pick).
//! Keydeck's, on Values' selection.

use egui::{Color32, RichText};
use kn_ui::theme;
use nm_core::kn_model::format::TextProp;

use crate::app::App;

/// The text style the Fonts window shows: the cursor cell's, else the selected text box's.
fn current(app: &App) -> Option<nm_core::kn_model::CharStyle> {
    if app.sel.is_some() {
        return crate::menus::cursor_char(app);
    }
    let o = app.canvas.obj?;
    let d = app.drawables.iter().find(|d| d.id == o)?;
    match &d.kind {
        nm_core::kn_model::DrawableKind::Shape(s) => s.text.as_ref().and_then(|t| t.paragraphs.first()).and_then(|p| p.runs.first()).map(|r| r.style.clone()),
        _ => None,
    }
}

pub fn fonts_window(app: &mut App, ctx: &egui::Context) {
    if !app.show_fonts {
        return;
    }
    let cur = current(app);
    let names = crate::editing::font_names(app);
    let mut open = true;
    let mut apply: Vec<(&'static str, TextProp)> = Vec::new();
    egui::Window::new("Fonts").default_pos(ctx.content_rect().center() - egui::vec2(260.0, 160.0)).collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| {
        let can = cur.is_some();
        if !can {
            ui.label(RichText::new("Select cells or a text box to change its font.").color(theme::TEXT_DIM()));
        }
        ui.add_enabled_ui(can, |ui| {
            let st = cur.clone().unwrap_or_default();
            ui.horizontal(|ui| {
                if ui.add(egui::Button::new(RichText::new("U").underline()).selected(st.underline)).on_hover_text("Underline").clicked() {
                    apply.push(("Underline", TextProp::Underline(!st.underline)));
                }
                if ui.add(egui::Button::new(RichText::new("S").strikethrough()).selected(st.strike)).on_hover_text("Strikethrough").clicked() {
                    apply.push(("Strikethrough", TextProp::Strike(!st.strike)));
                }
                let mut c = Color32::from_rgba_unmultiplied((st.color.r * 255.0) as u8, (st.color.g * 255.0) as u8, (st.color.b * 255.0) as u8, 255);
                if kn_ui::colors::color_well(ui, "fonts-text-color", &mut c, 64.0) {
                    apply.push(("Text Color", TextProp::Color(nm_core::kn_model::Color { r: c.r() as f32 / 255.0, g: c.g() as f32 / 255.0, b: c.b() as f32 / 255.0, a: 1.0 })));
                }
                ui.add(egui::TextEdit::singleline(&mut app.font_query).hint_text("Search").desired_width(f32::INFINITY));
            });
            ui.separator();
            let family = kn_render::fonts::parse_ps_name(&st.font_name).0;
            let q = app.font_query.to_lowercase();
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.label(theme::bold("Family"));
                    egui::ScrollArea::vertical().id_salt("font-family").max_height(240.0).max_width(220.0).show(ui, |ui| {
                        ui.set_width(210.0);
                        for n in names.iter().filter(|n| q.is_empty() || n.to_lowercase().contains(&q)) {
                            if ui.selectable_label(*n == family, n).clicked() {
                                apply.push(("Font", TextProp::Font(n.replace(' ', ""))));
                            }
                        }
                    });
                });
                ui.vertical(|ui| {
                    ui.label(theme::bold("Typeface"));
                    ui.set_width(120.0);
                    for (name, b, i) in [("Regular", false, false), ("Italic", false, true), ("Bold", true, false), ("Bold Italic", true, true)] {
                        if ui.selectable_label(st.bold == b && st.italic == i, name).clicked() {
                            if st.bold != b {
                                apply.push(("Bold", TextProp::Bold(b)));
                            }
                            if st.italic != i {
                                apply.push(("Italic", TextProp::Italic(i)));
                            }
                        }
                    }
                });
                ui.vertical(|ui| {
                    ui.label(theme::bold("Size"));
                    ui.set_width(80.0);
                    let mut sz = f64::from(st.size);
                    if ui.add(egui::DragValue::new(&mut sz).range(1.0..=1638.0).suffix(" pt")).changed() {
                        apply.push(("Font Size", TextProp::Size(sz as f32)));
                    }
                    egui::ScrollArea::vertical().id_salt("font-size").max_height(212.0).show(ui, |ui| {
                        for v in [9.0, 10.0, 11.0, 12.0, 13.0, 14.0, 18.0, 24.0, 36.0, 48.0, 64.0, 72.0, 96.0, 144.0, 288.0] {
                            if ui.selectable_label((st.size - v).abs() < 0.01, format!("{v:.0}")).clicked() {
                                apply.push(("Font Size", TextProp::Size(v)));
                            }
                        }
                    });
                });
            });
        });
    });
    app.show_fonts = open;
    for (label, p) in apply {
        crate::editing::set_text(app, label, p);
    }
}

/// Replace Fonts…: (font name in the file, installed here, chosen replacement family).
pub struct ReplaceFonts {
    rows: Vec<(String, bool, Option<String>)>,
}

pub fn open_replace(app: &mut App) {
    let Some(doc) = app.doc() else { return };
    let used = doc.fonts_used();
    let rows = used.into_iter().map(|(f, _)| {
        let installed = app.canvas.tiles.font_installed(&f);
        (f, installed, None)
    });
    app.replace_fonts = Some(ReplaceFonts { rows: rows.collect() });
}

pub fn replace_window(app: &mut App, ctx: &egui::Context) {
    let Some(mut st) = app.replace_fonts.take() else { return };
    let families = app.canvas.tiles.font_families();
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
                    ui.label(if *installed { RichText::new(name.as_str()) } else { RichText::new(format!("⚠ {name}")).color(Color32::from_rgb(230, 160, 60)) });
                    ui.label("→");
                    theme::combo(("replace", i)).width(220.0).selected_text(to.clone().unwrap_or_else(|| "Don't replace".into())).show_ui(ui, |ui| {
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
        let changes: Vec<(String, String)> = st.rows.iter().filter_map(|(from, _, to)| to.as_ref().map(|fam| (from.clone(), app.canvas.tiles.ps_name_for_family(fam).unwrap_or_else(|| fam.clone())))).collect();
        let n = app.edit("Replace Fonts", |d, h| changes.iter().map(|(from, to)| d.replace_font(h, from, to)).sum::<usize>()).unwrap_or(0);
        app.canvas.tiles.clear_text_caches();
        app.notify(format!("Replaced fonts in {n} style{}", if n == 1 { "" } else { "s" }));
        return;
    }
    if open {
        app.replace_fonts = Some(st);
    }
}

/// View > Show Warnings: Numbers' Document Warnings (fonts that aren't installed, media that
/// can't be found, what an Excel workbook can't keep), each with what to do about it.
pub fn open_warnings(app: &mut App) {
    let Some(doc) = app.doc() else { return };
    let mut list = Vec::new();
    let fonts: Vec<String> = doc.fonts_used().into_iter().map(|(f, _)| f).collect();
    let mut missing_media = Vec::new();
    if let Some(pm) = doc.file.package_metadata() {
        for d in pm.msgs("datas") {
            let Some(id) = d.u64("identifier") else { continue };
            if doc.file.data(id).is_none() {
                missing_media.push(d.str("preferred_file_name").or(d.str("file_name")).unwrap_or("media").to_string());
            }
        }
    }
    for f in fonts {
        if !app.canvas.tiles.font_installed(&f) {
            list.push(format!("The font \u{201c}{f}\u{201d} is missing and was replaced."));
        }
    }
    for m in missing_media {
        list.push(format!("\u{201c}{m}\u{201d} couldn\u{2019}t be found and is shown as a placeholder."));
    }
    if app.excel.as_ref().is_some_and(|x| !x.compat) {
        list.push("This is an Excel workbook: charts, shapes, text boxes, comments, categories and filters made here aren\u{2019}t saved in it. Choose Format: Excel + Values (click the title) to keep them.".into());
    }
    app.warnings = Some(list);
}

pub fn warnings_window(app: &mut App, ctx: &egui::Context) {
    let Some(list) = app.warnings.clone() else { return };
    let mut open = true;
    let mut replace = false;
    let missing_fonts = list.iter().any(|w| w.starts_with("The font"));
    egui::Window::new("Document Warnings").pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).collapsible(false).resizable(true).default_width(460.0).open(&mut open).show(ctx, |ui| {
        if list.is_empty() {
            ui.label(RichText::new("There are no warnings for this spreadsheet.").color(theme::TEXT_DIM()));
        }
        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
            for w in &list {
                ui.label(format!("\u{26a0}  {w}"));
            }
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if missing_fonts && ui.button("Replace Fonts\u{2026}").clicked() {
                replace = true;
            }
            if ui.button("Copy").clicked() {
                ui.ctx().copy_text(list.join("\n"));
            }
        });
    });
    if !open {
        app.warnings = None;
    }
    if replace {
        open_replace(app);
    }
}
