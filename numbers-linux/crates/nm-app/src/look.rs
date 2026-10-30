//! Values' look: the colour theme, light or dark, liquid glass, translucent panes, the classic
//! look and the window's title bar. Each app of the suite keeps its own (Values' in
//! ~/.config/numdeck/look.txt); the first time, Values starts from Keydeck's look.

use std::path::PathBuf;

/// Numbers' green: Values' Default colour theme.
pub const NUMBERS_GREEN: u32 = 0x2daa4f;

#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    pub dark: bool,
    pub color_theme: String,
    pub liquid_glass: bool,
    pub translucent_panes: bool,
    pub classic_look: bool,
    pub system_title_bar: bool,
}

// (on macOS the system's own title bar by default, as Keydeck)
impl Default for Look {
    fn default() -> Self {
        Look { dark: true, color_theme: "default".into(), liquid_glass: false, translucent_panes: false, classic_look: false, system_title_bar: cfg!(target_os = "macos") }
    }
}

fn config(app: &str, file: &str) -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join(app).join(file))
}

/// Values' own look file.
pub fn path() -> Option<PathBuf> {
    config("numdeck", "look.txt")
}

impl Look {
    /// Values' look (defaults where a setting is missing); before Values has one of its own,
    /// Keydeck's.
    pub fn load() -> Look {
        let mut l = Look::default();
        let text = path().and_then(|p| std::fs::read_to_string(p).ok()).or_else(|| config("keydeck", "settings.txt").and_then(|p| std::fs::read_to_string(p).ok()));
        let Some(text) = text else { return l };
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            let on = v == "true";
            match k {
                "dark" => l.dark = on,
                "color_theme" => l.color_theme = v.to_string(),
                "liquid_glass" => l.liquid_glass = on,
                "translucent_panes" => l.translucent_panes = on,
                "classic_look" => l.classic_look = on,
                "system_title_bar" => l.system_title_bar = on,
                _ => {}
            }
        }
        l
    }

    /// Style the interface: colour theme, appearance and glass (call once the context exists).
    pub fn apply(&self, ctx: &egui::Context) {
        // Values' Default is Numbers' green (Keydeck's is its blue)
        kn_ui::theme::set_default_accent(NUMBERS_GREEN);
        kn_ui::theme::set_color_theme(&self.color_theme);
        kn_ui::theme::apply_appearance(ctx, self.dark);
        // popovers, menus, combo lists and tooltips open in one frame (Mac N21: "adding fades
        // would feel less like Numbers"); egui fades every popup in over animation_time. The
        // inspector's slide keeps its own timing (kn_ui::theme::slide)
        ctx.all_styles_mut(|s| s.animation_time = 0.0);
        kn_ui::glass::set_enabled(self.liquid_glass);
        kn_ui::glass::set_translucent_panes(self.translucent_panes);
        kn_ui::glass::set_classic(self.classic_look);
    }
}

// Numbers' window colours in dark mode, measured on the Mac (N2 screenshots): the title bar
// with the toolbar #1e1e1e, the sheet tabs #262626, the toolbar's capsules #3c3c3c with a light
// rim; tinted by the colour theme as Keydeck's greys are. Light mode keeps Keydeck's.

/// The title bar and toolbar's background.
pub fn header_bg() -> egui::Color32 {
    if kn_ui::theme::is_light() { kn_ui::theme::TOOLBAR_BG() } else { kn_ui::theme::tint(egui::Color32::from_gray(0x1e)) }
}

/// The sheet tabs' strip.
pub fn tabs_bg() -> egui::Color32 {
    if kn_ui::theme::is_light() { kn_ui::theme::gray(0x24) } else { kn_ui::theme::tint(egui::Color32::from_gray(0x26)) }
}

/// A toolbar capsule's fill.
pub fn capsule_fill() -> egui::Color32 {
    if kn_ui::theme::is_light() { kn_ui::theme::tint(egui::Color32::from_gray(0xf4)) } else { kn_ui::theme::tint(egui::Color32::from_gray(0x3c)) }
}
