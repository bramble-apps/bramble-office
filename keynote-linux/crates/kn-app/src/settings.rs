//! Keynote > Settings (persisted in ~/.config/keydeck/settings.txt) and Help > shortcuts.

use std::path::PathBuf;

use egui::RichText;

use crate::app::{App, Zoom};

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub dark: bool,
    pub fit_zoom: bool,
    pub spell_check: bool,
    pub show_size_while_moving: bool,
    pub exit_after_last_slide: bool,
    /// Play > In Window (otherwise In Fullscreen).
    pub play_in_window: bool,
    /// Edit > Substitutions > Smart Quotes / Smart Dashes (on by default, as on a Mac).
    pub smart_quotes: bool,
    pub smart_dashes: bool,
    /// Window title bar: false = menus in the title bar (drawn by the app, default),
    /// true = the system's GNOME-style (Adwaita) title bar. Takes effect on next start.
    pub system_title_bar: bool,
    /// Rulers > Alignment Guides (Keynote's defaults: centre, spacing and sizing on, edges off).
    pub guides_center: bool,
    pub guides_edges: bool,
    pub guides_spacing: bool,
    pub guides_sizing: bool,
    /// What shows under the floating toolbar: "feathered" (Keydeck's own, default), "tinted"
    /// (as Pages) or "solid".
    pub toolbar_background: String,
    /// Rulers > Ruler Units: 0 points, 1 inches, 2 centimetres, 3 percent.
    pub ruler_units: u8,
    /// Settings > Scaling: percent, 0 = the system's (kn_ui::scale).
    pub ui_scale: u32,
    /// Settings > Layout (Android): 0 automatic, 1 phone, 2 tablet (kn_ui::form).
    pub ui_layout: u8,
    /// Rulers > Place origin at center of ruler.
    pub ruler_origin_center: bool,
    /// Slideshow > Show slideshow controls on pointer move / Show pointer only on slides with
    /// links or movies.
    pub show_pointer_always: bool,
    /// Slideshow > Scale slides up to fit display.
    pub scale_slides_up: bool,
    /// Edit > Spelling and Grammar / Substitutions.
    pub auto_correct: bool,
    pub grammar: bool,
    pub text_replacement: bool,
    pub smart_links: bool,
    pub smart_phone_links: bool,
    /// View > Comments > Author Color (index into AUTHOR_COLORS).
    pub author_color: u8,
    /// Play > Show Presenter Display in Window; Play > Customize Presenter Display.
    pub presenter_in_window: bool,
    pub pd_current: bool,
    pub pd_next: bool,
    pub pd_notes: bool,
    pub pd_clock: bool,
    pub pd_timer: bool,
    pub pd_ready: bool,
    /// View > Customize Toolbar: which toolbar items are shown.
    /// View > Customize Toolbar…: the toolbar's items in order (Keynote's identifiers), empty =
    /// the default set; display mode 1 icon and text, 2 icon only, 3 text only; shown or hidden.
    pub toolbar_items: String,
    /// The camera live video shows (a /dev/video node; empty: the first one).
    pub camera: String,
    /// The Colors window's saved colours (hex, comma-separated; shared shape with Numdeck).
    pub saved_colors: String,
    pub toolbar_mode: u8,
    /// Colour theme (theme::COLOR_THEMES id).
    pub color_theme: String,
    /// Liquid glass toolbar and tab buttons (off by default).
    pub liquid_glass: bool,
    /// Translucent navigator and Format pane (off by default).
    pub translucent_panes: bool,
    /// The layout from before the macOS 26 styling (toolbar strip, square tabs).
    pub classic_look: bool,
    pub toolbar_shown: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            dark: true,
            fit_zoom: true,
            spell_check: true,
            show_size_while_moving: true,
            exit_after_last_slide: true,
            play_in_window: false,
            smart_quotes: true,
            smart_dashes: true,
            // macOS: its own title bar and traffic lights by default (the integrated one is a
            // GNOME look)
            system_title_bar: cfg!(target_os = "macos"),
            guides_center: true,
            guides_edges: false,
            guides_spacing: true,
            guides_sizing: true,
            ruler_units: 0,
            ui_scale: 0,
            ui_layout: 0,
            toolbar_background: "feathered".into(),
            ruler_origin_center: false,
            show_pointer_always: true,
            scale_slides_up: true,
            auto_correct: false,
            grammar: true,
            text_replacement: true,
            smart_links: true,
            smart_phone_links: false,
            author_color: 0,
            presenter_in_window: false,
            pd_current: true,
            pd_next: true,
            pd_notes: true,
            pd_clock: true,
            pd_timer: true,
            pd_ready: true,
            toolbar_items: String::new(),
            camera: String::new(),
            saved_colors: String::new(),
            toolbar_mode: 2,
            color_theme: "default".into(),
            liquid_glass: false,
            translucent_panes: false,
            classic_look: false,
            toolbar_shown: true,
        }
    }
}

fn path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("keydeck").join("settings.txt"))
}

impl Settings {
    /// Every on/off setting with its name in settings.txt.
    fn flags(&mut self) -> [(&'static str, &mut bool); 32] {
        [
            ("classic_look", &mut self.classic_look),
            ("translucent_panes", &mut self.translucent_panes),
            ("liquid_glass", &mut self.liquid_glass),
            ("dark", &mut self.dark),
            ("fit_zoom", &mut self.fit_zoom),
            ("spell_check", &mut self.spell_check),
            ("show_size_while_moving", &mut self.show_size_while_moving),
            ("exit_after_last_slide", &mut self.exit_after_last_slide),
            ("play_in_window", &mut self.play_in_window),
            ("smart_quotes", &mut self.smart_quotes),
            ("smart_dashes", &mut self.smart_dashes),
            ("system_title_bar", &mut self.system_title_bar),
            ("guides_center", &mut self.guides_center),
            ("guides_edges", &mut self.guides_edges),
            ("guides_spacing", &mut self.guides_spacing),
            ("guides_sizing", &mut self.guides_sizing),
            ("ruler_origin_center", &mut self.ruler_origin_center),
            ("show_pointer_always", &mut self.show_pointer_always),
            ("scale_slides_up", &mut self.scale_slides_up),
            ("auto_correct", &mut self.auto_correct),
            ("grammar", &mut self.grammar),
            ("text_replacement", &mut self.text_replacement),
            ("smart_links", &mut self.smart_links),
            ("smart_phone_links", &mut self.smart_phone_links),
            ("presenter_in_window", &mut self.presenter_in_window),
            ("pd_current", &mut self.pd_current),
            ("pd_next", &mut self.pd_next),
            ("pd_notes", &mut self.pd_notes),
            ("pd_clock", &mut self.pd_clock),
            ("pd_timer", &mut self.pd_timer),
            ("pd_ready", &mut self.pd_ready),
            ("toolbar_shown", &mut self.toolbar_shown),
        ]
    }

    pub fn load() -> Settings {
        let mut s = Settings::default();
        let Some(text) = path().and_then(|p| std::fs::read_to_string(p).ok()) else { return s };
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            if k == "author_color" {
                s.author_color = v.parse().unwrap_or(0).min(9);
                continue;
            }
            if k == "color_theme" {
                s.color_theme = v.to_string();
                continue;
            }
            if k == "camera" {
                s.camera = v.to_string();
                continue;
            }
            if k == "saved_colors" {
                s.saved_colors = v.to_string();
                continue;
            }
            if k == "toolbar_items" {
                s.toolbar_items = v.to_string();
                continue;
            }
            if k == "toolbar_mode" {
                s.toolbar_mode = v.parse().unwrap_or(2).clamp(1, 3);
                continue;
            }
            if k == "toolbar_background" {
                s.toolbar_background = v.to_string();
                continue;
            }
            if k == "ui_layout" {
                s.ui_layout = v.parse().unwrap_or(0).min(2);
            }
            if k == "ui_scale" {
                s.ui_scale = v.parse().unwrap_or(0).min(400);
                continue;
            }
            if k == "ruler_units" {
                s.ruler_units = v.parse().unwrap_or(0).min(3);
                continue;
            }
            for (name, f) in s.flags() {
                if name == k {
                    *f = v == "true";
                }
            }
        }
        s
    }

    pub fn save(&self) {
        let Some(p) = path() else { return };
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let mut me = self.clone();
        let mut text: String = me.flags().iter().map(|(k, v)| format!("{k}={}\n", **v)).collect();
        text.push_str(&format!("ruler_units={}\n", self.ruler_units));
        text.push_str(&format!("ui_scale={}\n", self.ui_scale));
        text.push_str(&format!("ui_layout={}\n", self.ui_layout));
        text.push_str(&format!("toolbar_background={}\n", self.toolbar_background));
        text.push_str(&format!("author_color={}\n", self.author_color));
        text.push_str(&format!("toolbar_items={}\n", self.toolbar_items));
        text.push_str(&format!("camera={}\n", self.camera));
        text.push_str(&format!("saved_colors={}\n", self.saved_colors));
        text.push_str(&format!("toolbar_mode={}\n", self.toolbar_mode));
        text.push_str(&format!("color_theme={}\n", self.color_theme));
        let _ = std::fs::write(p, text);
    }
}

/// Keynote's comment author colours (View > Comments > Author Color), in menu order.
pub const AUTHOR_COLORS: [(&str, [f32; 3]); 10] = [
    ("Yellow", [0.980, 0.937, 0.353]),
    ("Green", [0.596, 0.882, 0.420]),
    ("Blue", [0.478, 0.745, 0.996]),
    ("Pink", [0.996, 0.580, 0.765]),
    ("Purple", [0.765, 0.616, 0.988]),
    ("Teal", [0.420, 0.871, 0.843]),
    ("Orange", [0.996, 0.706, 0.380]),
    ("Red", [0.996, 0.478, 0.478]),
    ("Brown", [0.808, 0.663, 0.502]),
    ("Gray Blue", [0.643, 0.722, 0.808]),
];

pub fn apply(ctx: &egui::Context, s: &Settings) {
    crate::theme::set_color_theme(&s.color_theme);
    crate::glass::set_enabled(s.liquid_glass);
    crate::glass::set_translucent_panes(s.translucent_panes);
    crate::glass::set_classic(s.classic_look);
    crate::glass::set_toolbar_background(crate::glass::ToolbarBackground::from_key(&s.toolbar_background).unwrap_or(crate::glass::ToolbarBackground::Feathered));
    crate::theme::apply_appearance(ctx, s.dark);
}

pub fn settings_window(app: &mut App, ctx: &egui::Context) {
    if !app.show_settings {
        return;
    }
    let mut open = true;
    let before = app.settings.clone();
    let mut tab = ctx.data(|d| d.get_temp::<usize>(egui::Id::unique("settings-tab"))).unwrap_or(0);
    egui::Window::new("Settings").pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).collapsible(false).resizable(false).open(&mut open).min_width(460.0).show(ctx, |ui| {
        // Keynote's settings toolbar: General, Slideshow, Rulers
        ui.horizontal(|ui| {
            for (i, name) in ["General", "Slideshow", "Rulers"].iter().enumerate() {
                if ui.selectable_label(tab == i, RichText::new(*name).size(13.0)).clicked() {
                    tab = i;
                }
            }
        });
        ui.separator();
        let s = &mut app.settings;
        match tab {
            0 => {
                egui::Grid::new("settings-general").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
                    ui.label("Appearance:");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut s.dark, false, "Light");
                        ui.radio_value(&mut s.dark, true, "Dark");
                    });
                    ui.end_row();
                    // colour themes: a swatch each (main colour, accent in the lower right)
                    ui.label("Color:");
                    ui.vertical(|ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                            for (id, name, main, accent) in crate::theme::COLOR_THEMES {
                                let (r, resp) = ui.allocate_exact_size(egui::vec2(26.0, 26.0), egui::Sense::click());
                                let p = ui.painter();
                                let c = r.center();
                                p.circle_filled(c, 11.0, crate::theme::rgb(main));
                                if accent != main {
                                    p.circle_filled(c + egui::vec2(5.5, 5.5), 4.5, crate::theme::rgb(accent));
                                }
                                if s.color_theme == id {
                                    p.circle_stroke(c, 12.5, egui::Stroke::new(2.0, crate::theme::TEXT()));
                                }
                                if resp.on_hover_text(name).clicked() {
                                    s.color_theme = id.to_string();
                                }
                            }
                        });
                        let name = crate::theme::COLOR_THEMES.iter().find(|t| t.0 == s.color_theme).map(|t| t.1).unwrap_or("Default");
                        ui.label(RichText::new(name).small().weak());
                    });
                    ui.end_row();
                    ui.label("Buttons:");
                    ui.vertical(|ui| {
                        ui.checkbox(&mut s.liquid_glass, "Glass (toolbar and inspector tabs)");
                        ui.checkbox(&mut s.translucent_panes, "Translucent sidebars (slide and desktop show through)");
                        ui.checkbox(&mut s.classic_look, "Classic look (toolbar strip and square tabs, as before)");
                    });
                    ui.end_row();
                    ui.label("Toolbar background:");
                    {
                        use crate::glass::ToolbarBackground as Tb;
                        let cur = Tb::from_key(&s.toolbar_background).unwrap_or(Tb::Feathered);
                        let name = Tb::ALL.iter().find(|a| a.0 == cur).map(|a| a.1).unwrap_or("");
                        crate::theme::combo("toolbar-background").selected_text(name).show_ui(ui, |ui| {
                            for (b, n) in Tb::ALL {
                                if ui.selectable_label(b == cur, n).clicked() {
                                    s.toolbar_background = b.key().into();
                                }
                            }
                        });
                    }
                    ui.end_row();
                    ui.label("Editing:");
                    ui.vertical(|ui| {
                        ui.checkbox(&mut s.show_size_while_moving, "Show size and position when moving objects");
                        ui.checkbox(&mut s.spell_check, "Check spelling while typing");
                        ui.checkbox(&mut s.smart_quotes, "Use smart quotes");
                        ui.checkbox(&mut s.smart_dashes, "Use smart dashes");
                    });
                    ui.end_row();
                    ui.label("Zoom:");
                    ui.checkbox(&mut s.fit_zoom, "Fit slide to window when opening");
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
                    if cfg!(any(target_os = "windows", target_os = "android")) {
                        return;
                    }
                    ui.label("Window:");
                    ui.vertical(|ui| {
                        ui.radio_value(&mut s.system_title_bar, false, "Menus in the title bar");
                        let system = if crate::winframe::desktop_draws_no_frames() { "System title bar (GNOME leaves frames to apps: a basic one)" } else { "System title bar (the system's own window frame)" };
                        ui.radio_value(&mut s.system_title_bar, true, system);
                        ui.label(RichText::new("Takes effect the next time Keydeck starts.").small().weak());
                    });
                    ui.end_row();
                });
            }
            1 => {
                egui::Grid::new("settings-show").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
                    ui.label("Presentation:");
                    ui.vertical(|ui| {
                        ui.radio_value(&mut s.play_in_window, false, "Play in full screen");
                        ui.radio_value(&mut s.play_in_window, true, "Play in window");
                        ui.checkbox(&mut s.scale_slides_up, "Scale slides up to fit display");
                        ui.checkbox(&mut s.exit_after_last_slide, "Exit presentation after last slide");
                    });
                    ui.end_row();
                    ui.label("Pointer:");
                    ui.vertical(|ui| {
                        ui.radio_value(&mut s.show_pointer_always, true, "Show pointer when using the mouse or trackpad");
                        ui.radio_value(&mut s.show_pointer_always, false, "Show pointer only on slides with links or movies");
                    });
                    ui.end_row();
                });
            }
            _ => {
                egui::Grid::new("settings-rulers").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
                    ui.label("Ruler Units:");
                    let names = ["Points", "Inches", "Centimeters", "Percent"];
                    crate::theme::combo("ruler-units").selected_text(names[s.ruler_units as usize]).show_ui(ui, |ui| {
                        for (i, n) in names.iter().enumerate() {
                            ui.selectable_value(&mut s.ruler_units, i as u8, *n);
                        }
                    });
                    ui.end_row();
                    ui.label("");
                    ui.checkbox(&mut s.ruler_origin_center, "Place origin at center of ruler");
                    ui.end_row();
                    ui.label("Alignment Guides:");
                    ui.vertical(|ui| {
                        ui.checkbox(&mut s.guides_center, "Show guides at object center");
                        ui.checkbox(&mut s.guides_edges, "Show guides at object edges");
                        ui.checkbox(&mut s.guides_spacing, "Show guides for relative spacing");
                        ui.checkbox(&mut s.guides_sizing, "Show guides for relative sizing");
                    });
                    ui.end_row();
                });
            }
        }
    });
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique("settings-tab"), tab));
    app.show_settings = open;
    if app.settings != before {
        app.settings.save();
        apply(ctx, &app.settings);
        if app.settings.fit_zoom {
            app.zoom = Zoom::Fit;
        }
    }
}

const SHORTCUTS: &[(&str, &str)] = &[
    ("Play slideshow", "Ctrl+Alt+P"),
    ("Next / previous slide (slideshow)", "Space, →  /  ←"),
    ("Exit slideshow", "Esc"),
    ("New slide", "Ctrl+Shift+N"),
    ("Skip slide", "Ctrl+Shift+H"),
    ("Edit slide layouts", "Ctrl+Shift+E"),
    ("Undo / Redo", "Ctrl+Z  /  Ctrl+Shift+Z"),
    ("Select all / Deselect all", "Ctrl+A  /  Ctrl+Shift+A"),
    ("Delete selection", "Delete or Backspace"),
    ("Move selection", "Arrow keys (Shift = 10 pt)"),
    ("Constrain move / keep proportions", "Shift while dragging"),
    ("Rotate", "Ctrl+drag a handle"),
    ("Edit text", "Double-click a text box"),
    ("New line / line break", "Enter  /  Shift+Enter"),
    ("Finish editing text", "Esc or click outside"),
    ("Find & Replace", "Ctrl+F"),
    ("Bring to front / Send to back", "Ctrl+Shift+F  /  Ctrl+Shift+B"),
    ("Lock / Unlock", "Ctrl+L  /  Ctrl+Alt+L"),
    ("Presenter notes", "Ctrl+Shift+P"),
    ("Rulers", "Ctrl+R"),
    ("Open / Save / Save As", "Ctrl+O  /  Ctrl+S  /  Ctrl+Shift+S"),
];

pub fn help_window(app: &mut App, ctx: &egui::Context) {
    if !app.show_help {
        return;
    }
    let mut open = true;
    egui::Window::new("Keyboard Shortcuts").pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center()).collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| {
        egui::Grid::new("shortcuts").num_columns(2).spacing([24.0, 6.0]).striped(true).show(ui, |ui| {
            for (what, keys) in SHORTCUTS {
                ui.label(*what);
                ui.label(RichText::new(*keys).monospace());
                ui.end_row();
            }
        });
    });
    app.show_help = open;
}
