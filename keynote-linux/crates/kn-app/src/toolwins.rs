//! Floating tool windows: View > Show Arrange Tools, Show Colors (Keynote's colour panel), and
//! Format > Font > Show Fonts (opens the Text inspector).

use egui::{Color32, RichText};

use crate::app::App;
use crate::theme;

#[derive(Default, Debug)]
pub struct ToolWindows {
    pub arrange: bool,
    pub colors: bool,
    color: Option<Color32>,
    /// Format > Font > Show Fonts: the Fonts window, and what its search box holds.
    pub fonts: bool,
    font_query: String,
    /// The shared Apple-style colour panel (kn-ui): its mode and saved colours.
    pub color_panel: kn_ui::colors::ColorPanelState,
    color_panel_loaded: bool,
}

/// Format > Font > Show Fonts: the Fonts window (as the Mac's font panel: family, typeface,
/// size, underline / strikethrough and text colour, applied to the selection as you pick).
pub fn show_fonts(app: &mut App) {
    app.tools.fonts = !app.tools.fonts;
}

fn fonts_window(app: &mut App, ctx: &egui::Context) {
    use kn_model::format::TextProp;
    if !app.tools.fonts {
        return;
    }
    let cur = crate::editing::current_run(app);
    if app.font_names.is_none() {
        app.font_names = Some(crate::editing::font_names(app));
    }
    let names = app.font_names.clone().unwrap_or_default();
    let mut open = true;
    let mut apply: Vec<(&'static str, TextProp)> = Vec::new();
    egui::Window::new("Fonts").default_pos(ctx.content_rect().center() - egui::vec2(260.0, 160.0)).collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| {
        let can = cur.is_some();
        if !can {
            ui.label(RichText::new("Select text or a text box to change its font.").color(theme::TEXT_DIM()));
        }
        ui.add_enabled_ui(can, |ui| {
            let st = cur.clone().unwrap_or_default();
            // underline, strikethrough, colour
            ui.horizontal(|ui| {
                if ui.add(egui::Button::new(RichText::new("U").underline()).selected(st.underline)).on_hover_text("Underline").clicked() {
                    apply.push(("Underline", TextProp::Underline(!st.underline)));
                }
                if ui.add(egui::Button::new(RichText::new("S").strikethrough()).selected(st.strike)).on_hover_text("Strikethrough").clicked() {
                    apply.push(("Strikethrough", TextProp::Strike(!st.strike)));
                }
                let mut c = Color32::from_rgba_unmultiplied((st.color.r * 255.0) as u8, (st.color.g * 255.0) as u8, (st.color.b * 255.0) as u8, 255);
                if kn_ui::colors::color_well(ui, "fonts-text-color", &mut c, 64.0) {
                    apply.push(("Text Color", TextProp::Color(kn_model::Color { r: c.r() as f32 / 255.0, g: c.g() as f32 / 255.0, b: c.b() as f32 / 255.0, a: 1.0 })));
                }
                ui.add(egui::TextEdit::singleline(&mut app.tools.font_query).hint_text("Search").desired_width(f32::INFINITY));
            });
            ui.separator();
            let family = kn_render::fonts::parse_ps_name(&st.font_name).0;
            let q = app.tools.font_query.to_lowercase();
            ui.horizontal_top(|ui| {
                // Family
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
                // Typeface
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
                // Size
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
    app.tools.fonts = open;
    for (label, p) in apply {
        crate::editing::set_text(app, label, p);
    }
}

pub fn ui(app: &mut App, ctx: &egui::Context) {
    fonts_window(app, ctx);
    // the Colors window an inspector colour well's wheel button opened; saved colours persist
    if !app.tools.color_panel_loaded {
        app.tools.color_panel.saved_from_string(&app.settings.saved_colors);
        app.tools.color_panel_loaded = true;
    }
    kn_ui::colors::colors_window(ctx, &mut app.tools.color_panel);
    let saved = app.tools.color_panel.saved_to_string();
    if saved != app.settings.saved_colors {
        app.settings.saved_colors = saved;
        app.settings.save();
    }
    if app.show_pd_custom {
        let mut open = true;
        let before = app.settings.clone();
        egui::Window::new("Customize Presenter Display").pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| {
            let s = &mut app.settings;
            ui.checkbox(&mut s.pd_current, "Current Slide");
            ui.checkbox(&mut s.pd_next, "Next Slide");
            ui.checkbox(&mut s.pd_notes, "Presenter Notes");
            ui.checkbox(&mut s.pd_clock, "Clock");
            ui.checkbox(&mut s.pd_timer, "Timer");
            ui.checkbox(&mut s.pd_ready, "Ready to Advance indicator");
            ui.separator();
            ui.checkbox(&mut s.presenter_in_window, "Show presenter display in a window");
        });
        app.show_pd_custom = open;
        if app.settings != before {
            app.settings.save();
        }
    }
    if app.tools.arrange {
        let mut open = true;
        let mut fire: Option<&str> = None;
        egui::Window::new("Arrange").pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| {
            let groups: [&[(&str, &str)]; 5] = [
                &[("Front", "Bring to Front"), ("Forward", "Bring Forward"), ("Backward", "Send Backward"), ("Back", "Send to Back")],
                &[("Left", "Align Objects > Left"), ("Center", "Align Objects > Center"), ("Right", "Align Objects > Right")],
                &[("Top", "Align Objects > Top"), ("Middle", "Align Objects > Middle"), ("Bottom", "Align Objects > Bottom")],
                &[("Distribute H", "Distribute Objects > Horizontally"), ("Distribute V", "Distribute Objects > Vertically"), ("Flip H", "Flip Horizontally"), ("Flip V", "Flip Vertically")],
                &[("Group", "Group"), ("Ungroup", "Ungroup"), ("Lock", "Lock"), ("Unlock", "Unlock")],
            ];
            for g in groups {
                ui.horizontal(|ui| {
                    for (label, cmd) in g {
                        if ui.add_enabled(crate::menus::enabled(app, cmd), egui::Button::new(*label).min_size(egui::vec2(64.0, 24.0))).clicked() {
                            fire = Some(cmd);
                        }
                    }
                });
            }
        });
        app.tools.arrange = open;
        if let Some(c) = fire {
            app.command(ctx, c);
        }
    }
    if app.tools.colors {
        let mut open = true;
        let mut fill = false;
        let mut text = false;
        // (the suite's Colors window: its frame, margins and eyedropper, kn-ui)
        let mut c = app.tools.color.unwrap_or(Color32::from_rgb(0x1e, 0x88, 0xe5));
        let (has_sel, editing) = (!app.sel.is_empty(), app.canvas.is_editing_text());
        kn_ui::colors::colors_window_with(ctx, "kd-show-colors", &mut app.tools.color_panel, &mut c, &mut open, |ui| {
            ui.horizontal(|ui| {
                fill = ui.add_enabled(has_sel, egui::Button::new("Apply to Fill")).clicked();
                text = ui.add_enabled(has_sel || editing, egui::Button::new("Apply to Text")).clicked();
            });
            ui.label(RichText::new("Select objects or text, then apply.").small().color(theme::TEXT_DIM()));
        });
        app.tools.color = Some(c);
        app.tools.colors = open;
        let c = app.tools.color.unwrap_or(Color32::WHITE);
        let col = kn_model::Color { r: c.r() as f32 / 255.0, g: c.g() as f32 / 255.0, b: c.b() as f32 / 255.0, a: c.a() as f32 / 255.0 };
        if fill {
            let f = kn_model::format::FillSpec::Color(col);
            crate::editing::apply(app, "Fill", |doc, h, id| doc.set_fill(h, id, &f));
        }
        if text {
            crate::editing::set_text(app, "Text Color", kn_model::format::TextProp::Color(col));
        }
    }
}
