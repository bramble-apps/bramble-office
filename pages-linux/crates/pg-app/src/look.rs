//! Letters' look: the colour theme, light or dark, liquid glass, translucent panes, the classic
//! look, the window's title bar and the toolbar's background. Each app of the suite keeps its own
//! (Letters' in ~/.config/letters/look.txt); the first time, Letters starts from Keydeck's.

use std::path::PathBuf;

/// Pages' orange (Mac research: segmented control selection #F7A23E-ish): Letters' Default
/// colour theme.
pub const PAGES_ORANGE: u32 = 0xf7a23e;

#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    pub dark: bool,
    pub color_theme: String,
    pub liquid_glass: bool,
    pub translucent_panes: bool,
    pub classic_look: bool,
    pub system_title_bar: bool,
    /// What shows under the floating toolbar (Pages: the document blurred under a tint).
    pub toolbar_background: kn_ui::glass::ToolbarBackground,
}

// (on macOS the system's own title bar by default, as Keydeck)
impl Default for Look {
    fn default() -> Self {
        Look { dark: true, color_theme: "default".into(), liquid_glass: false, translucent_panes: false, classic_look: false, system_title_bar: cfg!(target_os = "macos"), toolbar_background: kn_ui::glass::ToolbarBackground::Tinted }
    }
}

fn config(app: &str, file: &str) -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join(app).join(file))
}

/// Letters' own look file.
pub fn path() -> Option<PathBuf> {
    config("letters", "look.txt")
}

impl Look {
    /// Letters' look (defaults where a setting is missing); before it has one of its own,
    /// Keydeck's (but Pages' toolbar background, Letters' default).
    pub fn load() -> Look {
        let mut l = Look::default();
        let own = path().and_then(|p| std::fs::read_to_string(p).ok());
        let seeded = own.is_none();
        let Some(text) = own.or_else(|| config("keydeck", "settings.txt").and_then(|p| std::fs::read_to_string(p).ok())) else { return l };
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
                "toolbar_background" if !seeded => {
                    if let Some(b) = kn_ui::glass::ToolbarBackground::from_key(v) {
                        l.toolbar_background = b;
                    }
                }
                _ => {}
            }
        }
        l
    }

    /// Style the interface: colour theme, appearance and glass (call once the context exists).
    pub fn apply(&self, ctx: &egui::Context) {
        // Letters' Default is Pages' orange (Keydeck's is its blue, Values' green)
        kn_ui::theme::set_default_accent(PAGES_ORANGE);
        kn_ui::theme::set_color_theme(&self.color_theme);
        kn_ui::theme::apply_appearance(ctx, self.dark);
        // popovers, menus, combo lists and tooltips open in one frame (Mac N21); egui fades every popup in over animation_time. The
        // inspector's slide keeps its own timing (kn_ui::theme::slide)
        ctx.all_styles_mut(|s| s.animation_time = 0.0);
        // dialogs and popovers cast a soft shadow around them, a little lower than higher (macOS
        // sheets), not egui's hard block offset down and right
        let dark = self.dark;
        ctx.all_styles_mut(|s| {
            s.visuals.window_shadow = egui::Shadow { offset: [0, 10], blur: 36, spread: 0, color: egui::Color32::from_black_alpha(if dark { 120 } else { 50 }) };
            s.visuals.popup_shadow = egui::Shadow { offset: [0, 4], blur: 16, spread: 0, color: egui::Color32::from_black_alpha(if dark { 90 } else { 36 }) };
        });
        kn_ui::glass::set_enabled(self.liquid_glass);
        kn_ui::glass::set_translucent_panes(self.translucent_panes);
        kn_ui::glass::set_classic(self.classic_look);
        kn_ui::glass::set_toolbar_background(self.toolbar_background);
    }
}

// Numbers' window colours in dark mode, measured on the Mac (N2 screenshots): the title bar
// with the toolbar #1e1e1e, the sheet tabs #262626, the toolbar's capsules #3c3c3c with a light
// rim; tinted by the colour theme as Keydeck's greys are. Light mode keeps Keydeck's.

/// The title bar and toolbar's background.
pub fn header_bg() -> egui::Color32 {
    if kn_ui::theme::is_light() { kn_ui::theme::TOOLBAR_BG() } else { kn_ui::theme::tint(egui::Color32::from_gray(0x1e)) }
}

/// Behind the pages: a soft grey in both modes, light enough that the page reads as paper on a desk
/// rather than a hole (`theme::gray` inverts values in light mode, so these are explicit).
pub fn canvas_bg() -> egui::Color32 {
    if kn_ui::theme::is_light() { kn_ui::theme::tint(egui::Color32::from_gray(0xe9)) } else { kn_ui::theme::tint(egui::Color32::from_gray(0x2b)) }
}

/// The inspector's paragraph style button: fill and label colour.
pub fn style_button_colors() -> (egui::Color32, egui::Color32) {
    if kn_ui::theme::is_light() {
        (kn_ui::theme::tint(egui::Color32::from_gray(0xe2)), egui::Color32::from_gray(0x1e))
    } else {
        (kn_ui::theme::tint(egui::Color32::from_gray(0x4c)), egui::Color32::from_gray(0xf0))
    }
}

/// A toolbar capsule's fill.
pub fn capsule_fill() -> egui::Color32 {
    if kn_ui::theme::is_light() { kn_ui::theme::tint(egui::Color32::from_gray(0xf4)) } else { kn_ui::theme::tint(egui::Color32::from_gray(0x3c)) }
}
