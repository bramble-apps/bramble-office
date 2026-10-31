//! Letters > Settings… (Pages' General / Rulers / Auto-Correction, with the look settings).
//! The look (appearance, colour theme, buttons, title bar, toolbar background) is Letters' own
//! (~/.config/letters/look.txt), apart from Keydeck's; the rest is
//! ~/.config/letters/settings.txt.

use std::path::PathBuf;

use egui::RichText;
use kn_ui::theme;

use crate::app::App;
use crate::look::Look;

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub show_size_while_moving: bool,
    pub autocomplete_cells: bool,
    pub formula_suggestions: bool,
    pub author: String,
    /// Zoom of a new sheet (Numbers: 125 %).
    pub default_zoom: f32,
    pub ruler_units: u8,
    /// Settings > Scaling: percent, 0 = the system's (kn_ui::scale).
    pub ui_scale: u32,
    /// Settings > Layout (Android): 0 automatic, 1 phone, 2 tablet (kn_ui::form).
    pub ui_layout: u8,
    pub guides_center: bool,
    pub guides_edges: bool,
    pub guides_spacing: bool,
    pub guides_sizing: bool,
    pub smart_quotes: bool,
    pub smart_dashes: bool,
    pub capitalize: bool,
    /// View > Author Color: index into AUTHOR_COLORS (your comments' colour).
    pub author_color: u8,
    /// The Colors window's saved colours (hex values).
    pub saved_colors: String,
    /// Edit > Substitutions > Text Replacement, and Spelling and Grammar's switches.
    pub text_replacement: bool,
    pub spell_check: bool,
    pub grammar: bool,
    pub autocorrect: bool,
    /// Settings > General > For New Documents: show the template chooser.
    pub show_chooser: bool,
    /// Default Font for new Basic documents (family, "" = the template's) and size.
    pub default_font: String,
    pub default_font_size: f32,
    /// Auto-Correction > Detect lists.
    pub detect_lists: bool,
    /// View > Customize Toolbar…: the toolbar's items in order (empty: Pages' default set).
    pub toolbar_items: String,
    /// Toolbar display: 1 icon and text, 2 icon only, 3 text only.
    pub toolbar_mode: u8,
}

/// View > Author Color's colours, in the menu's order.
pub const AUTHOR_COLORS: [(&str, u32); 10] = [
    ("Yellow", 0xf5c518),
    ("Green", 0x33d17a),
    ("Blue", 0x3584e4),
    ("Pink", 0xe66ba4),
    ("Purple", 0x9141ac),
    ("Teal", 0x26a69a),
    ("Orange", 0xff7800),
    ("Red", 0xe01b24),
    ("Brown", 0x986a44),
    ("Gray Blue", 0x6a8caf),
];

impl Default for Settings {
    fn default() -> Self {
        Settings {
            show_size_while_moving: true,
            autocomplete_cells: true,
            formula_suggestions: true,
            author: std::env::var("USER").unwrap_or_default(),
            default_zoom: 1.25,
            ruler_units: 0,
            ui_scale: 0,
            ui_layout: 0,
            guides_center: true,
            guides_edges: true,
            guides_spacing: true,
            guides_sizing: true,
            smart_quotes: true,
            smart_dashes: true,
            capitalize: false,
            author_color: 0,
            saved_colors: String::new(),
            text_replacement: true,
            spell_check: true,
            grammar: false,
            autocorrect: false,
            show_chooser: true,
            default_font: String::new(),
            default_font_size: 11.0,
            detect_lists: true,
            toolbar_items: String::new(),
            toolbar_mode: 2,
        }
    }
}

fn config_dir(app: &str) -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join(app))
}

impl Settings {
    pub fn load() -> Settings {
        let mut s = Settings::default();
        let Some(text) = config_dir("letters").and_then(|d| std::fs::read_to_string(d.join("settings.txt")).ok()) else { return s };
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            let on = v == "true";
            match k {
                "show_size_while_moving" => s.show_size_while_moving = on,
                "autocomplete_cells" => s.autocomplete_cells = on,
                "formula_suggestions" => s.formula_suggestions = on,
                "author" => s.author = v.to_string(),
                "default_zoom" => s.default_zoom = v.parse().unwrap_or(1.25),
                "ruler_units" => s.ruler_units = v.parse().unwrap_or(0).min(3),
                "ui_scale" => s.ui_scale = v.parse().unwrap_or(0).min(400),
                "ui_layout" => s.ui_layout = v.parse().unwrap_or(0).min(2),
                "guides_center" => s.guides_center = on,
                "guides_edges" => s.guides_edges = on,
                "guides_spacing" => s.guides_spacing = on,
                "guides_sizing" => s.guides_sizing = on,
                "smart_quotes" => s.smart_quotes = on,
                "smart_dashes" => s.smart_dashes = on,
                "capitalize" => s.capitalize = on,
                "author_color" => s.author_color = v.parse().unwrap_or(0).min(9),
                "saved_colors" => s.saved_colors = v.to_string(),
                "text_replacement" => s.text_replacement = on,
                "spell_check" => s.spell_check = on,
                "grammar" => s.grammar = on,
                "autocorrect" => s.autocorrect = on,
                "toolbar_items" => s.toolbar_items = v.to_string(),
                "show_chooser" => s.show_chooser = on,
                "default_font" => s.default_font = v.to_string(),
                "default_font_size" => s.default_font_size = v.parse().unwrap_or(11.0),
                "detect_lists" => s.detect_lists = on,
                "toolbar_mode" => s.toolbar_mode = v.parse().unwrap_or(2).clamp(1, 3),
                _ => {}
            }
        }
        s
    }

    pub fn save(&self) {
        let Some(dir) = config_dir("letters") else { return };
        let _ = std::fs::create_dir_all(&dir);
        let text = format!(
            "show_size_while_moving={}\nautocomplete_cells={}\nformula_suggestions={}\nauthor={}\ndefault_zoom={}\nruler_units={}\nguides_center={}\nguides_edges={}\nguides_spacing={}\nguides_sizing={}\nsmart_quotes={}\nsmart_dashes={}\ncapitalize={}\nauthor_color={}\nsaved_colors={}\ntext_replacement={}\nspell_check={}\ngrammar={}\nautocorrect={}\ntoolbar_items={}\ntoolbar_mode={}\nshow_chooser={}\ndefault_font={}\ndefault_font_size={}\ndetect_lists={}\nui_scale={}\nui_layout={}\n",
            self.show_size_while_moving, self.autocomplete_cells, self.formula_suggestions, self.author, self.default_zoom, self.ruler_units, self.guides_center, self.guides_edges, self.guides_spacing, self.guides_sizing, self.smart_quotes, self.smart_dashes, self.capitalize, self.author_color, self.saved_colors, self.text_replacement, self.spell_check, self.grammar, self.autocorrect, self.toolbar_items, self.toolbar_mode, self.show_chooser, self.default_font, self.default_font_size, self.detect_lists, self.ui_scale, self.ui_layout
        );
        let _ = std::fs::write(dir.join("settings.txt"), text);
    }
}

impl Look {
    /// Write the look to Letters' own file (Keydeck and the other apps keep theirs).
    pub fn save(&self) {
        let Some(path) = crate::look::path() else { return };
        let text = format!(
            "dark={}\ncolor_theme={}\nliquid_glass={}\ntranslucent_panes={}\nclassic_look={}\nsystem_title_bar={}\ntoolbar_background={}\n",
            self.dark, self.color_theme, self.liquid_glass, self.translucent_panes, self.classic_look, self.system_title_bar, self.toolbar_background.key()
        );
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, text);
    }
}

/// The Settings window (Letters > Settings…, Ctrl+,).
pub fn window(app: &mut App, ctx: &egui::Context) {
    if !app.show_settings {
        return;
    }
    let mut open = true;
    let (look0, s0) = (app.look.clone(), app.settings.clone());
    let key = egui::Id::unique("settings-tab");
    let mut tab: usize = ctx.data(|d| d.get_temp(key)).unwrap_or(0);
    egui::Window::new("Settings").pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).collapsible(false).resizable(false).open(&mut open).min_width(500.0).show(ctx, |ui| {
        ui.horizontal(|ui| {
            for (i, name) in ["General", "Rulers", "Auto-Correction"].iter().enumerate() {
                if ui.selectable_label(tab == i, RichText::new(*name).size(13.0)).clicked() {
                    tab = i;
                }
            }
        });
        ui.separator();
        let (l, s) = (&mut app.look, &mut app.settings);
        match tab {
            0 => {
                egui::Grid::new("settings-general").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
                    ui.label("Appearance:");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut l.dark, false, "Light");
                        ui.radio_value(&mut l.dark, true, "Dark");
                    });
                    ui.end_row();
                    ui.label("Color:");
                    ui.vertical(|ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                            for (id, name, main, accent) in theme::COLOR_THEMES {
                                // Default is Letters' own orange
                                let (main, accent) = if id == "default" { (theme::default_accent(), theme::default_accent()) } else { (main, accent) };
                                let (r, resp) = ui.allocate_exact_size(egui::vec2(26.0, 26.0), egui::Sense::click());
                                let p = ui.painter();
                                let c = r.center();
                                p.circle_filled(c, 11.0, theme::rgb(main));
                                if accent != main {
                                    p.circle_filled(c + egui::vec2(5.5, 5.5), 4.5, theme::rgb(accent));
                                }
                                if l.color_theme == id {
                                    p.circle_stroke(c, 12.5, egui::Stroke::new(2.0, theme::TEXT()));
                                }
                                if resp.on_hover_text(if id == "default" { "Pages Orange" } else { name }).clicked() {
                                    l.color_theme = id.to_string();
                                }
                            }
                        });
                        let name = theme::COLOR_THEMES.iter().skip(1).find(|t| t.0 == l.color_theme).map(|t| t.1).unwrap_or("Pages Orange");
                        ui.label(RichText::new(format!("{name} (shared with Keydeck)")).small().weak());
                    });
                    ui.end_row();
                    ui.label("Buttons:");
                    ui.vertical(|ui| {
                        theme::checkbox(ui, &mut l.liquid_glass, "Glass (toolbar and inspector tabs)");
                        theme::checkbox(ui, &mut l.translucent_panes, "Translucent inspector");
                        theme::checkbox(ui, &mut l.classic_look, "Classic look (toolbar strip)");
                    });
                    ui.end_row();
                    ui.label("Toolbar:");
                    let cur = kn_ui::glass::ToolbarBackground::ALL.iter().find(|a| a.0 == l.toolbar_background).map(|a| a.1).unwrap_or("");
                    theme::combo("toolbar-background").selected_text(cur).width(180.0).show_ui(ui, |ui| {
                        for (b, name) in kn_ui::glass::ToolbarBackground::ALL {
                            ui.selectable_value(&mut l.toolbar_background, b, name);
                        }
                    });
                    ui.end_row();
                    ui.label("Scaling:");
                    kn_ui::scale::combo(ui, "ui-scale", &mut s.ui_scale);
                    ui.end_row();
                    // (Android: the phone or tablet layout, or by the window's width)
                    if cfg!(target_os = "android") {
                        ui.label("Layout:");
                        kn_ui::form::combo(ui, "ui-layout", &mut s.ui_layout);
                        ui.end_row();
                    }
                    // (Windows: always the app's own title bar, with the menus in it)
                    if !cfg!(any(target_os = "windows", target_os = "android")) {
                        ui.label("Window:");
                        ui.vertical(|ui| {
                            ui.radio_value(&mut l.system_title_bar, false, "Menus in the title bar");
                            ui.radio_value(&mut l.system_title_bar, true, "System title bar");
                            ui.label(RichText::new("Takes effect the next time Letters starts.").small().weak());
                        });
                        ui.end_row();
                    }
                    ui.label("Editing:");
                    ui.vertical(|ui| {
                        theme::checkbox(ui, &mut s.show_size_while_moving, "Show size and position when moving objects");
                        theme::checkbox(ui, &mut s.formula_suggestions, "Show suggestions when editing table cells");
                    });
                    ui.end_row();
                    ui.label("For New Documents:");
                    ui.vertical(|ui| {
                        ui.radio_value(&mut s.show_chooser, true, "Show Template Chooser");
                        ui.radio_value(&mut s.show_chooser, false, "Use template: Blank");
                    });
                    ui.end_row();
                    ui.label("Default Font:");
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut s.default_font).hint_text("Helvetica Neue").desired_width(150.0));
                        theme::stepper32(ui, &mut s.default_font_size, 1.0, 6.0..=96.0, " pt", 0);
                    });
                    ui.end_row();
                    ui.label("Default Zoom:");
                    ui.horizontal(|ui| {
                        ui.label("Zoom");
                        theme::combo("default-zoom").selected_text(format!("{:.0}%", s.default_zoom * 100.0)).show_ui(ui, |ui| {
                            for z in crate::chrome::ZOOMS {
                                ui.selectable_value(&mut s.default_zoom, z, format!("{:.0}%", z * 100.0));
                            }
                        });
                    });
                    ui.end_row();
                    ui.label("Author:");
                    ui.add(egui::TextEdit::singleline(&mut s.author).hint_text("Name shown on comments").desired_width(240.0));
                    ui.end_row();
                });
            }
            1 => {
                egui::Grid::new("settings-rulers").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
                    ui.label("Ruler Units:");
                    let names = ["Centimeters", "Inches", "Points", "Millimeters"];
                    theme::combo("ruler-units").selected_text(names[s.ruler_units as usize]).show_ui(ui, |ui| {
                        for (i, n) in names.iter().enumerate() {
                            ui.selectable_value(&mut s.ruler_units, i as u8, *n);
                        }
                    });
                    ui.end_row();
                    ui.label("Alignment Guides:");
                    ui.vertical(|ui| {
                        theme::checkbox(ui, &mut s.guides_center, "Show guides at object center");
                        theme::checkbox(ui, &mut s.guides_edges, "Show guides at object edges");
                        theme::checkbox(ui, &mut s.guides_spacing, "Show guides for relative spacing");
                        theme::checkbox(ui, &mut s.guides_sizing, "Show guides for relative sizing");
                    });
                    ui.end_row();
                });
            }
            _ => {
                egui::Grid::new("settings-autocorrect").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
                    ui.label("Formatting:");
                    ui.vertical(|ui| {
                        theme::checkbox(ui, &mut s.smart_quotes, "Use smart quotes “ ”");
                        theme::checkbox(ui, &mut s.smart_dashes, "Use smart dashes —");
                        theme::checkbox(ui, &mut s.capitalize, "Capitalize words automatically");
                        theme::checkbox(ui, &mut s.text_replacement, "Use text replacement ((c) \u{2192} \u{a9}, -> \u{2192} \u{2192})");
                        theme::checkbox(ui, &mut s.detect_lists, "Automatically detect lists");
                    });
                    ui.end_row();
                    ui.label("Spelling:");
                    ui.vertical(|ui| {
                        theme::checkbox(ui, &mut s.spell_check, "Check spelling while typing");
                        theme::checkbox(ui, &mut s.grammar, "Check grammar with spelling");
                        theme::checkbox(ui, &mut s.autocorrect, "Correct spelling automatically");
                    });
                    ui.end_row();
                });
            }
        }
    });
    ctx.data_mut(|d| d.insert_temp(key, tab));
    app.show_settings = open;
    if app.look != look0 {
        app.look.apply(ctx);
        app.look.save();
    }
    if app.settings != s0 {
        app.settings.save();
    }
}
