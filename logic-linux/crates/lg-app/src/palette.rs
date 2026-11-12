//! Bramble's custom colour palette (beyond Logic 13): the track / region Color palette can be
//! edited, saved to a file and loaded from one (Studio's own `.bspalette` text format, see
//! lg_core::trackmix::palette_text), so a palette can be shared. The palette in use is kept in
//! Studio's settings and applies to every project; Reset goes back to the built-in colours.
//! In the Colors window: Edit Palette (a click on a colour then edits it), Save Palette…,
//! Load Palette…, Reset Palette.

use std::path::PathBuf;

use egui::Color32;
use lg_core::trackmix::{self, PALETTE_COLS, PALETTE_ROWS};

use crate::app::App;

pub const EXT: &str = "bspalette";

/// The menu items this file runs (Bramble's own: not in Logic's lists).
pub const COMMANDS: &[&str] = &[];

#[derive(Clone, Debug, Default)]
pub struct State {
    /// Edit Palette is on: the colour being edited.
    pub editing: bool,
    pub selected: Option<u8>,
    pub name: String,
}

fn saved_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("bramble-studio").join(format!("palette.{EXT}")))
}

/// Every colour of the palette in use.
pub fn current() -> Vec<[u8; 3]> {
    (0..(PALETTE_COLS * PALETTE_ROWS) as u8).map(trackmix::palette).collect()
}

/// At start: the palette saved last time (not in automated runs, unless LG_PALETTE names a
/// palette file to use).
pub fn load_saved(automated: bool) -> String {
    let path = match std::env::var_os("LG_PALETTE") {
        Some(p) => Some(PathBuf::from(p)),
        None if automated => None,
        None => saved_path(),
    };
    let Some((name, cols)) = path.and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| trackmix::parse_palette(&t)) else { return String::new() };
    trackmix::set_custom_palette(Some(cols));
    name
}

fn persist(app: &App) {
    if app.automated {
        return;
    }
    let Some(p) = saved_path() else { return };
    match trackmix::custom_palette() {
        Some(_) => {
            if let Some(d) = p.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let _ = std::fs::write(p, trackmix::palette_text(&app.pal.name, &current()));
        }
        None => {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// Sets one colour (the palette becomes a custom one).
pub fn set_color(app: &mut App, k: u8, c: [u8; 3]) {
    let mut v = current();
    if let Some(x) = v.get_mut(k as usize) {
        *x = c;
    }
    trackmix::set_custom_palette(Some(v));
    persist(app);
}

pub fn reset(app: &mut App) {
    trackmix::set_custom_palette(None);
    app.pal.name.clear();
    persist(app);
}

/// Loads a palette file; false when it isn't one.
pub fn load_file(app: &mut App, path: &std::path::Path) -> bool {
    let Some((name, cols)) = std::fs::read_to_string(path).ok().and_then(|t| trackmix::parse_palette(&t)) else { return false };
    trackmix::set_custom_palette(Some(cols));
    app.pal.name = if name.is_empty() { path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default() } else { name };
    persist(app);
    true
}

pub fn save_file(app: &App, path: &std::path::Path) -> std::io::Result<()> {
    let name = if app.pal.name.is_empty() { path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default() } else { app.pal.name.clone() };
    std::fs::write(path, trackmix::palette_text(&name, &current()))
}

/// A click on swatch `k` in the Colors window: true when Edit Palette took it (the colour is
/// then edited, not assigned).
pub fn swatch_clicked(app: &mut App, k: u8) -> bool {
    if app.pal.editing {
        app.pal.selected = Some(k);
        return true;
    }
    false
}

/// Whether swatch `k` is the one being edited (drawn with a ring).
pub fn is_selected(app: &App, k: u8) -> bool {
    app.pal.editing && app.pal.selected == Some(k)
}

/// The Colors window's palette controls (under its colours).
pub fn controls(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        if ui.add(egui::Button::new("Edit Palette").selected(app.pal.editing)).on_hover_text("A click on a colour edits it instead of assigning it").clicked() {
            app.pal.editing = !app.pal.editing;
        }
        if ui.button("Load Palette…").clicked() && !app.automated {
            if let Some(p) = kn_ui::files::FileDialog::new().set_title("Load Palette").add_filter("Bramble Studio Palette", &[EXT]).pick_file() {
                if !load_file(app, &p) {
                    app.tell("That file isn't a Bramble Studio palette");
                }
            }
        }
        if ui.button("Save Palette…").clicked() && !app.automated {
            if let Some(mut p) = kn_ui::files::FileDialog::new().set_title("Save Palette").add_filter("Bramble Studio Palette", &[EXT]).set_file_name(format!("Palette.{EXT}")).save_file() {
                if p.extension().is_none() {
                    p.set_extension(EXT);
                }
                if let Err(e) = save_file(app, &p) {
                    app.tell(format!("Couldn't save the palette: {e}"));
                }
            }
        }
        if ui.add_enabled(trackmix::custom_palette().is_some(), egui::Button::new("Reset Palette")).on_hover_text("Back to the built-in colours").clicked() {
            reset(app);
        }
    });
    if !app.pal.editing {
        return;
    }
    let Some(k) = app.pal.selected else {
        ui.label(egui::RichText::new("Click a colour to edit it").weak());
        return;
    };
    let [r, g, b] = trackmix::palette(k);
    let mut c = Color32::from_rgb(r, g, b);
    ui.horizontal(|ui| {
        ui.label(format!("Colour {} (row {}, column {})", k as usize + 1, k as usize / PALETTE_COLS + 1, k as usize % PALETTE_COLS + 1));
        let mut hex = format!("{r:02x}{g:02x}{b:02x}");
        if ui.add(egui::TextEdit::singleline(&mut hex).desired_width(60.0)).changed() {
            let h = hex.trim_start_matches('#');
            if h.len() == 6 {
                if let (Ok(r), Ok(g), Ok(b)) = (u8::from_str_radix(&h[0..2], 16), u8::from_str_radix(&h[2..4], 16), u8::from_str_radix(&h[4..6], 16)) {
                    set_color(app, k, [r, g, b]);
                }
            }
        }
    });
    if egui::color_picker::color_picker_color32(ui, &mut c, egui::color_picker::Alpha::Opaque) {
        set_color(app, k, [c.r(), c.g(), c.b()]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_palettes_load_back() {
        let dir = std::env::temp_dir().join(format!("bs-palette-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let f = dir.join("mine.bspalette");
        let mut cols: Vec<[u8; 3]> = (0..96u8).map(trackmix::default_palette).collect();
        cols[0] = [9, 8, 7];
        std::fs::write(&f, trackmix::palette_text("Mine", &cols)).unwrap();
        let (n, back) = trackmix::parse_palette(&std::fs::read_to_string(&f).unwrap()).unwrap();
        assert_eq!(n, "Mine");
        assert_eq!(back[0], [9, 8, 7]);
        assert_eq!(back.len(), PALETTE_COLS * PALETTE_ROWS);
        let _ = std::fs::remove_dir_all(dir);
    }
}
